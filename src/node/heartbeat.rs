//! Peer heartbeats and failure suspicion.
//!
//! Raft heartbeats only flow from the leader to followers, so a follower cannot tell when
//! another follower (for example a bidding winner) goes silent. Every node sends
//! `PeerBody::Heartbeat` to every peer each `timers.peer_heartbeat_ms` and suspects a peer after
//! `timers.takeover_missed_heartbeats` intervals with nothing heard from it (D41, D79).
//!
//! "Heard from" means its heartbeat arrived, or it answered ours with anything: a `StaleTerm`
//! answer still proves the peer is up. Suspicions change once per interval, in [`Suspicion::update`],
//! and each change is logged (`peer_suspected`, `peer_trusted`) for E3. While this node is
//! silenced it is the one cut off, so it evaluates nothing; on restore the clock of every peer it
//! was not already suspecting starts fresh (D151).
//!
//! Owner: C (faults and failover).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};

use crate::metrics::Event;
use crate::node::NodeContext;
use crate::proto::{NodeId, PeerBody};

/// When each peer was last heard from, and who is suspected. Takes the time as an argument so it
/// is easy to test.
#[derive(Debug, Clone)]
pub struct Suspicion {
    interval: Duration,
    missed: u32,
    last_heard: BTreeMap<NodeId, Instant>,
    /// Suspected peers and when the suspicion started.
    suspected: BTreeMap<NodeId, Instant>,
}

/// A change in what this node believes about one peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Nothing heard for `silent`.
    Suspected { peer: NodeId, silent: Duration },
    /// Heard from again after being suspected for `suspected_for`.
    Trusted {
        peer: NodeId,
        suspected_for: Duration,
    },
}

impl Suspicion {
    pub fn new(interval: Duration, missed: u32) -> Self {
        Self {
            interval,
            missed,
            last_heard: BTreeMap::new(),
            suspected: BTreeMap::new(),
        }
    }

    /// How long a peer may stay silent before it is suspected.
    pub fn threshold(&self) -> Duration {
        self.interval * self.missed
    }

    /// Start each peer's clock at `now`, as if just heard from, except a peer already suspected.
    /// Coming back from this node's own silence says nothing about that peer, so re-trusting it
    /// would list a dead machine again until it is suspected over again (D151).
    pub fn reset(&mut self, peers: impl IntoIterator<Item = NodeId>, now: Instant) {
        for peer in peers {
            if !self.suspected.contains_key(&peer) {
                self.last_heard.insert(peer, now);
            }
        }
    }

    /// Called for every heartbeat received and every reply to ours.
    pub fn heard_from(&mut self, peer: NodeId, at: Instant) {
        let last = self.last_heard.entry(peer).or_insert(at);
        *last = (*last).max(at);
    }

    /// Re-evaluate every known peer at `now` and return what changed.
    pub fn update(&mut self, now: Instant) -> Vec<Change> {
        let mut changes = Vec::new();
        for (&peer, &heard) in &self.last_heard {
            let silent = now.saturating_duration_since(heard);
            let over = silent >= self.threshold();
            match (over, self.suspected.get(&peer)) {
                (true, None) => {
                    self.suspected.insert(peer, now);
                    changes.push(Change::Suspected { peer, silent });
                }
                (false, Some(&since)) => {
                    self.suspected.remove(&peer);
                    changes.push(Change::Trusted {
                        peer,
                        suspected_for: now.saturating_duration_since(since),
                    });
                }
                _ => {}
            }
        }
        changes
    }

    /// True while `peer` is suspected, as of the last `update`.
    pub fn is_suspected(&self, peer: NodeId) -> bool {
        self.suspected.contains_key(&peer)
    }

    /// Every suspected peer, as of the last `update`.
    pub fn suspected(&self) -> BTreeSet<NodeId> {
        self.suspected.keys().copied().collect()
    }
}

/// Start sending heartbeats to every peer.
pub fn spawn(ctx: &Arc<NodeContext>) -> Option<JoinHandle<()>> {
    let ctx = ctx.clone();
    Some(tokio::spawn(async move {
        let interval = ctx.config.timers.peer_heartbeat();
        let peers: Vec<_> = ctx.peers.iter().cloned().collect();
        lock(&ctx.suspicion).reset(peers.iter().map(|p| p.peer()), Instant::now());
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut was_silenced = false;
        loop {
            tick.tick().await;
            if ctx.gate.is_silenced() {
                was_silenced = true;
                continue;
            }
            if was_silenced {
                was_silenced = false;
                lock(&ctx.suspicion).reset(peers.iter().map(|p| p.peer()), Instant::now());
            }

            let term = ctx.consensus.status().term;
            let answers = ctx
                .peers
                .broadcast(term, interval, || PeerBody::Heartbeat)
                .await;

            let now = Instant::now();
            let changes = {
                let mut s = lock(&ctx.suspicion);
                for (peer, answer) in answers {
                    // Any answer, even StaleTerm, proves the peer is up.
                    if answer.is_ok() {
                        s.heard_from(peer, now);
                    }
                }
                s.update(now)
            };
            for change in changes {
                log_change(&ctx, change);
            }
        }
    }))
}

fn lock(m: &Mutex<Suspicion>) -> MutexGuard<'_, Suspicion> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn log_change(ctx: &NodeContext, change: Change) {
    match change {
        Change::Suspected { peer, silent } => {
            let silent_ms = silent.as_millis() as u64;
            tracing::info!(node = ctx.id, peer, silent_ms, "peer suspected");
            ctx.log.log(Event::PeerSuspected { peer, silent_ms });
        }
        Change::Trusted {
            peer,
            suspected_for,
        } => {
            let suspected_ms = suspected_for.as_millis() as u64;
            tracing::info!(node = ctx.id, peer, suspected_ms, "peer trusted again");
            ctx.log.log(Event::PeerTrusted { peer, suspected_ms });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEAT: Duration = Duration::from_millis(50);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn an_unknown_peer_is_never_suspected() {
        let mut s = Suspicion::new(BEAT, 3);
        assert!(s.update(Instant::now() + ms(10_000)).is_empty());
        assert!(!s.is_suspected(2));
    }

    #[test]
    fn suspected_after_the_missed_intervals_and_trusted_once_heard() {
        let t0 = Instant::now();
        let mut s = Suspicion::new(BEAT, 3);
        s.reset([2, 3], t0);
        s.heard_from(3, t0 + ms(140));

        assert!(s.update(t0 + ms(149)).is_empty());
        let changes = s.update(t0 + ms(150));
        assert_eq!(
            changes,
            vec![Change::Suspected {
                peer: 2,
                silent: ms(150)
            }]
        );
        assert!(s.is_suspected(2));
        assert!(!s.is_suspected(3));
        // Still suspected: no second event.
        assert!(
            s.update(t0 + ms(200))
                .iter()
                .all(|c| !matches!(c, Change::Suspected { peer: 2, .. }))
        );

        s.heard_from(2, t0 + ms(400));
        let changes = s.update(t0 + ms(400));
        assert!(changes.contains(&Change::Trusted {
            peer: 2,
            suspected_for: ms(250)
        }));
        assert!(!s.is_suspected(2));
    }

    #[test]
    fn an_older_heartbeat_never_moves_the_clock_back() {
        let t0 = Instant::now();
        let mut s = Suspicion::new(BEAT, 3);
        s.heard_from(2, t0 + ms(100));
        s.heard_from(2, t0);
        assert!(s.update(t0 + ms(240)).is_empty());
    }

    #[test]
    fn reset_gives_a_peer_nobody_suspects_a_fresh_clock() {
        let t0 = Instant::now();
        let mut s = Suspicion::new(BEAT, 3);
        s.reset([2], t0);
        s.update(t0 + ms(100));
        assert!(s.suspected().is_empty());
        // 500 ms after it was last heard, which would be over the limit on the old clock.
        s.reset([2], t0 + ms(500));
        assert!(s.update(t0 + ms(500)).is_empty());
        assert!(s.suspected().is_empty());
    }

    #[test]
    fn a_suspected_peer_stays_suspected_through_reset() {
        let t0 = Instant::now();
        let mut s = Suspicion::new(BEAT, 3);
        s.reset([2, 3], t0);
        s.update(t0 + ms(500));
        assert_eq!(s.suspected(), BTreeSet::from([2, 3]));

        // Coming back from this node's own silence says nothing about those peers.
        s.reset([2, 3], t0 + ms(1_000));
        assert!(s.update(t0 + ms(1_000)).is_empty());
        assert_eq!(s.suspected(), BTreeSet::from([2, 3]));

        // Only hearing from one ends its suspicion.
        s.heard_from(2, t0 + ms(1_010));
        assert_eq!(
            s.update(t0 + ms(1_010)),
            vec![Change::Trusted {
                peer: 2,
                suspected_for: ms(510)
            }]
        );
        assert_eq!(s.suspected(), BTreeSet::from([3]));
    }
}
