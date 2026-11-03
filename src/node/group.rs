//! Group communication: the cloud nodes as one named group, with a who's-alive member list.
//!
//! [`CloudGroup`] knows the members from the config and keeps this node's own list of who is in
//! the group, its [`View`]. A caller sends once; the group makes the copies and reports who got
//! them (D152).
//!
//! - [`CloudGroup::send`] addresses the peers in the view. Bids use it, and so does anything else
//!   that only needs the peers this node believes are alive.
//! - [`CloudGroup::probe`] addresses every configured peer, in the view or not. Heartbeats use it:
//!   if they followed the list, two nodes that removed each other would never hear each other
//!   again, and a 20 s fault would last for ever (D154).
//!
//! **The view (D153).** A peer leaves after `timers.degraded_after_ms` of unbroken suspicion and
//! comes back the moment it is heard, so a one-tick false alarm changes nothing, and a fault under
//! about 2.2 s never shows. The heartbeat loop is the only thing that changes the view, at most
//! once per tick, so two peers changing together make one view. Each change logs `group_view`.
//!
//! **What it is not.** Each node decides alone: nobody agrees on the list, lists differ between
//! nodes, and a view number means nothing outside this process. Members means reachable, not able
//! to serve; `NodeStatus::serving` says that. The configured nodes are fixed and all stay Raft
//! voters (D25), so the view never reaches openraft, a `Command` or the replicated state.
//!
//! Owner: C (faults and failover).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::lock;
use super::transport::{PeerSet, TransportError};
use crate::config::ClusterConfig;
use crate::metrics::{Event, EventLog};
use crate::proto::{NodeId, PeerBody, PeerReply};
use crate::raft::Consensus;

/// This node's list of who is in the group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// Counts this process's changes, from 1. It restarts with the process and is never compared
    /// with another node's.
    pub number: u64,
    pub members: BTreeSet<NodeId>,
}

/// The cloud nodes as one group. One per node, shared through `Arc`.
pub struct CloudGroup {
    me: NodeId,
    all: BTreeSet<NodeId>,
    peers: Arc<PeerSet>,
    consensus: Arc<dyn Consensus>,
    log: EventLog,
    view: Mutex<View>,
}

impl CloudGroup {
    /// Everyone is in the first view, as every peer's clock starts fresh at start (D79).
    pub fn new(
        me: NodeId,
        cluster: &ClusterConfig,
        peers: Arc<PeerSet>,
        consensus: Arc<dyn Consensus>,
        log: EventLog,
    ) -> Self {
        let all: BTreeSet<NodeId> = cluster.nodes.iter().map(|n| n.id).collect();
        let view = View {
            number: 1,
            members: all.clone(),
        };
        Self {
            me,
            all,
            peers,
            consensus,
            log,
            view: Mutex::new(view),
        }
    }

    pub fn view(&self) -> View {
        lock(&self.view).clone()
    }

    /// One copy to every peer in the view, all at once, so a slow peer holds up nobody else. Best
    /// effort and unordered, with no retry (D126): anything that must be agreed goes through Raft.
    ///
    /// Returns one entry per addressed peer, to be read like this:
    ///
    /// - `Ok(Ack)`: the peer took it. **Only this counts as delivered.**
    /// - `Ok(StaleTerm)`: the peer answered but dropped it, by the term rule.
    /// - `Err(Timeout)` or `Err(Closed)`: unknown. It may still have arrived.
    /// - `Err(Backoff)`, `Err(Connect)` or `Err(Silenced)`: it was never sent.
    ///
    /// The view is this node's opinion, so it says who to send to, never who had to answer: a
    /// round that needs every node closes on `config.cluster.nodes`, never on the view.
    pub async fn send(
        &self,
        timeout: Duration,
        body: impl Fn() -> PeerBody,
    ) -> Vec<(NodeId, Result<PeerReply, TransportError>)> {
        let term = self.consensus.status().term;
        let members = self.view().members;
        self.peers
            .broadcast_to(|peer| members.contains(&peer), term, timeout, body)
            .await
    }

    /// The same as [`send`](Self::send), to every configured peer whatever the view says, so a
    /// peer off the list can still come back (D154).
    pub async fn probe(
        &self,
        timeout: Duration,
        body: impl Fn() -> PeerBody,
    ) -> Vec<(NodeId, Result<PeerReply, TransportError>)> {
        let term = self.consensus.status().term;
        self.peers.broadcast(term, timeout, body).await
    }

    /// Logs the view everyone starts in. Called from the heartbeat loop and not from `new`, which
    /// runs before the node logs `started`.
    pub(crate) fn log_first_view(&self) {
        self.log_view(&self.view());
    }

    /// Takes `left` out of the view and puts everyone else back. The one place the view changes.
    pub(crate) fn update(&self, left: &BTreeSet<NodeId>) {
        let changed = {
            let mut view = lock(&self.view);
            let next = next_view(&view, &self.all, self.me, left);
            if let Some(next) = &next {
                *view = next.clone();
            }
            next
        };
        if let Some(view) = changed {
            self.log_view(&view);
        }
    }

    fn log_view(&self, view: &View) {
        let members: Vec<NodeId> = view.members.iter().copied().collect();
        tracing::info!(node = self.me, view = view.number, ?members, "group view");
        self.log.log(Event::GroupView {
            view: view.number,
            members,
        });
    }
}

/// The view once `left` have left, or `None` if that is the view already. This node never removes
/// itself: being cut off is something its own silence and degraded mode deal with.
fn next_view(
    current: &View,
    all: &BTreeSet<NodeId>,
    me: NodeId,
    left: &BTreeSet<NodeId>,
) -> Option<View> {
    let members: BTreeSet<NodeId> = all
        .iter()
        .copied()
        .filter(|id| *id == me || !left.contains(id))
        .collect();
    (members != current.members).then(|| View {
        number: current.number + 1,
        members,
    })
}

#[cfg(test)]
mod tests {
    use tokio::time::Instant;

    use super::*;
    use crate::node::heartbeat::Suspicion;

    const HOLD: Duration = Duration::from_millis(2_000);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn set(ids: &[NodeId]) -> BTreeSet<NodeId> {
        ids.iter().copied().collect()
    }

    fn first() -> View {
        View {
            number: 1,
            members: set(&[1, 2, 3]),
        }
    }

    /// `next_view` as seen from node 1 of three.
    fn next(current: &View, left: &BTreeSet<NodeId>) -> Option<View> {
        next_view(current, &set(&[1, 2, 3]), 1, left)
    }

    #[test]
    fn the_number_rises_only_on_a_change() {
        let one = first();
        assert_eq!(next(&one, &set(&[])), None);

        let two = next(&one, &set(&[3])).unwrap();
        assert_eq!((two.number, two.members.clone()), (2, set(&[1, 2])));
        assert_eq!(next(&two, &set(&[3])), None);

        let three = next(&two, &set(&[])).unwrap();
        assert_eq!((three.number, three.members), (3, set(&[1, 2, 3])));
    }

    #[test]
    fn two_peers_leaving_in_one_tick_make_one_view() {
        let two = next(&first(), &set(&[2, 3])).unwrap();
        assert_eq!((two.number, two.members), (2, set(&[1])));
    }

    #[test]
    fn a_node_never_removes_itself() {
        assert_eq!(next(&first(), &set(&[1])), None);
        let alone = next(&first(), &set(&[1, 2, 3])).unwrap();
        assert_eq!(alone.members, set(&[1]));
    }

    #[test]
    fn a_peer_under_the_hold_stays() {
        let t0 = Instant::now();
        let mut s = Suspicion::new(ms(50), 3);
        s.reset([2], t0);
        s.update(t0 + ms(200));
        assert!(s.is_suspected(2));

        // Suspected, but a moment short of the hold: still a member.
        assert_eq!(next(&first(), &s.suspected_for(HOLD, t0 + ms(2_199))), None);

        let two = next(&first(), &s.suspected_for(HOLD, t0 + ms(2_200))).unwrap();
        assert_eq!(two.members, set(&[1, 3]));
    }

    #[test]
    fn a_heard_peer_returns_at_once() {
        let t0 = Instant::now();
        let mut s = Suspicion::new(ms(50), 3);
        s.reset([2], t0);
        s.update(t0 + ms(200));
        let two = next(&first(), &s.suspected_for(HOLD, t0 + ms(5_000))).unwrap();
        assert_eq!(two.members, set(&[1, 3]));

        // The tick that hears it is the tick that lists it: there is no hold on the way back.
        s.heard_from(2, t0 + ms(5_050));
        s.update(t0 + ms(5_050));
        let three = next(&two, &s.suspected_for(HOLD, t0 + ms(5_050))).unwrap();
        assert_eq!(three.members, set(&[1, 2, 3]));
    }
}
