//! The presence service: a cloud node's side of the directory (Phase 2, Part G, D287–D292).
//!
//! The application layer, like the incident service (`service.rs`): it takes what it needs from
//! below through `src/middleware.rs`, and `tests/layers.rs` holds it to that.
//!
//! - **Writes (`register`, `renew`, `deregister`).** A client sends each to every node (D327). One
//!   node proposes it: the member of this node's view that the user hashes to (D287). The others
//!   stay quiet, which means "not me" (D36), and the client takes the one answer. The proposer
//!   stamps the command with its clock and its `[presence]` lease (D281, D284), commits it, and
//!   answers from the outcome. Commit, then reply, as for reports.
//! - **Reads (`discover`).** Every serving node answers from its own applied state, at the
//!   directory's log clock, labelled with the index it read at (D288).
//! - **The tick (D281).** While the directory holds records, the lowest member of this node's view
//!   proposes `PresenceTick` every `tick_ms`, so leases run out with no client traffic.
//! - **Convergence (D293).** Every node that gets a `register` or `deregister` watches its own
//!   applied directory until the change shows, and logs `directory_seen` with the wait, on its own
//!   clock from the call arriving. The client sends to all three at once, so the slowest of the three
//!   waits is how long the presence table took to agree, with no two clocks compared.
//! - **A report a second (D294).** `directory_state`: records, live records, the log clock, expiries
//!   and the directory's own digest, for the plots and to compare nodes at one index.
//! - **Quiet when it should be.** A node that is not serving answers nothing (D46, D136), and a node
//!   left alone does not take writes, which could not commit (D120). It still answers `discover`
//!   while degraded: a read labelled with its index is true whatever the peers do.
//!
//! Owner: G (discovery directory).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::metrics::{Event, now_ms};
use crate::middleware::{NodeContext, PresenceAnswer, PresenceCall};
use crate::proto::{
    Command, DiscoverReply, Outcome, PresenceLease, PresenceOutcome, PresenceReply, UserId,
};
use crate::state::sample_mix;

/// Handle one presence call. `None` means stay quiet: the call stays open until the client cancels
/// it or its deadline passes, as for a report this node does not plan (D36).
pub(super) async fn handle(ctx: Arc<NodeContext>, call: PresenceCall) -> Option<PresenceAnswer> {
    if !ctx.serving.load(Ordering::SeqCst) {
        return None;
    }
    let started = Instant::now();
    let (user, kind) = match &call {
        PresenceCall::Discover(query) => {
            let (records, log_index) = ctx
                .state
                .with(|s| (s.directory().discover(query), s.last_index()));
            ctx.log.log(Event::DirectoryQuery {
                zone: query.zone,
                capability: query.capability,
                records: records.len(),
                log_index,
                latency_us: started.elapsed().as_micros() as u64,
            });
            return Some(PresenceAnswer::Discover(DiscoverReply::Found {
                records,
                log_index,
            }));
        }
        PresenceCall::Register(record) => (record.user, "register"),
        PresenceCall::Renew { user, .. } => (*user, "renew"),
        PresenceCall::Deregister { user, .. } => (*user, "deregister"),
    };
    if ctx.degraded.is_active() {
        return None;
    }
    let proposer = proposes_for(&ctx, user);
    if let Some(change) = Change::of(&call) {
        if !change.shows(&ctx) {
            tokio::spawn(watch(ctx.clone(), change, proposer, started));
        }
    }
    if !proposer {
        return None;
    }
    let command = command(&ctx, call)?;
    let outcome = match ctx.group.send_ordered(command).await {
        Ok(Outcome::Presence(outcome)) => outcome,
        Ok(other) => {
            tracing::warn!(%user, "unexpected outcome {} for a presence {kind}", other.kind());
            return None;
        }
        // The client's retry covers it, as for a report.
        Err(e) => {
            tracing::debug!(%user, "presence {kind} not committed: {e}");
            return None;
        }
    };
    // It may have stopped serving while it waited on the commit (D136).
    if !ctx.serving.load(Ordering::SeqCst) {
        return None;
    }
    ctx.log.log(Event::DirectoryWrite {
        user,
        kind,
        outcome: outcome.kind(),
        log_index: outcome.log_index(),
        latency_us: started.elapsed().as_micros() as u64,
    });
    Some(PresenceAnswer::Reply(reply(outcome)))
}

/// A change a write makes, which a node can see in its own directory (D293). A renewal moves only a
/// lease end the node cannot predict, so it is not watched.
enum Change {
    Registered(crate::proto::PresenceRecord),
    Deregistered { user: UserId, incarnation: u64 },
}

impl Change {
    fn of(call: &PresenceCall) -> Option<Self> {
        match call {
            PresenceCall::Register(record) => Some(Change::Registered(record.clone())),
            PresenceCall::Deregister { user, incarnation } => Some(Change::Deregistered {
                user: *user,
                incarnation: *incarnation,
            }),
            PresenceCall::Renew { .. } | PresenceCall::Discover(_) => None,
        }
    }

    fn shows(&self, ctx: &NodeContext) -> bool {
        ctx.state.with(|s| match self {
            Change::Registered(record) => s
                .directory()
                .entry(&record.user)
                .is_some_and(|e| e.record == *record),
            Change::Deregistered { user, incarnation } => s
                .directory()
                .entry(user)
                .is_none_or(|e| e.record.incarnation != *incarnation),
        })
    }

    fn user(&self) -> UserId {
        match self {
            Change::Registered(record) => record.user,
            Change::Deregistered { user, .. } => *user,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Change::Registered(_) => "register",
            Change::Deregistered { .. } => "deregister",
        }
    }
}

/// How often a node looks for a change it was told of, and how long it looks at most (D293).
const WATCH_EVERY: std::time::Duration = std::time::Duration::from_millis(2);
const WATCH_FOR: std::time::Duration = std::time::Duration::from_secs(10);

/// Wait until this node's directory shows `change`, and log how long that took from the call
/// arriving. A change that never shows (refused, or replaced first) logs nothing.
async fn watch(ctx: Arc<NodeContext>, change: Change, proposer: bool, started: Instant) {
    let deadline = started + WATCH_FOR;
    let mut tick = tokio::time::interval(WATCH_EVERY);
    while Instant::now() < deadline {
        tick.tick().await;
        if change.shows(&ctx) {
            ctx.log.log(Event::DirectorySeen {
                user: change.user(),
                kind: change.kind(),
                proposer,
                log_index: ctx.state.with(|s| s.last_index()),
                wait_us: started.elapsed().as_micros() as u64,
            });
            return;
        }
    }
}

/// Logs `directory_state` once a second (D294).
pub(super) fn spawn_report(ctx: &Arc<NodeContext>) -> JoinHandle<()> {
    let ctx = ctx.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tick.tick().await;
            let event = ctx.state.with(|s| {
                let d = s.directory();
                Event::DirectoryState {
                    last_applied: s.last_index(),
                    digest: d.content_digest(),
                    records: d.len(),
                    live: d.live_count(),
                    clock_ms: d.clock_ms(),
                    expired: d.counters().expired,
                    superseded: d.counters().superseded,
                    revived: d.counters().revived,
                }
            });
            ctx.log.log(event);
        }
    })
}

/// The command for a write, stamped with this node's clock and lease (D281, D284). A read has none.
fn command(ctx: &NodeContext, call: PresenceCall) -> Option<Command> {
    let settings = &ctx.config.presence;
    let lease = PresenceLease {
        at_ms: now_ms(),
        lease_ms: settings.lease_ms,
        grace_ms: settings.grace_ms,
    };
    Some(match call {
        PresenceCall::Register(record) => Command::PresenceRegister { record, lease },
        PresenceCall::Renew { user, incarnation } => Command::PresenceRenew {
            user,
            incarnation,
            lease,
        },
        PresenceCall::Deregister { user, incarnation } => Command::PresenceDeregister {
            user,
            incarnation,
            at_ms: lease.at_ms,
        },
        PresenceCall::Discover(_) => return None,
    })
}

/// What the client is told (D283, D286). A deregistration leaves no record, which is what `Unknown`
/// says; the client's `leave` takes either answer as done (D331).
fn reply(outcome: PresenceOutcome) -> PresenceReply {
    match outcome {
        PresenceOutcome::Registered {
            lease_ms,
            log_index,
            ..
        }
        | PresenceOutcome::Renewed {
            lease_ms,
            log_index,
            ..
        } => PresenceReply::Registered {
            lease_ms,
            log_index,
        },
        PresenceOutcome::Superseded { held, .. } => {
            PresenceReply::Rejected(format!("superseded by incarnation {held}"))
        }
        PresenceOutcome::Unknown { .. }
        | PresenceOutcome::Deregistered { .. }
        | PresenceOutcome::Ticked { .. } => PresenceReply::Unknown,
    }
}

/// Whether this node proposes the user's writes: the member of its view the user hashes to (D287).
/// The view holds this node, so the answer is never "nobody".
pub(super) fn proposes_for(ctx: &NodeContext, user: UserId) -> bool {
    let members = ctx.group.view().members;
    proposer(&members.iter().copied().collect::<Vec<_>>(), user) == Some(ctx.id)
}

/// The member of `members` (in ID order) that proposes `user`'s writes.
pub(super) fn proposer(members: &[u64], user: UserId) -> Option<u64> {
    if members.is_empty() {
        return None;
    }
    let hash = sample_mix(sample_mix(user.client) ^ u64::from(user.user));
    members.get((hash % members.len() as u64) as usize).copied()
}

/// Proposes `PresenceTick` every `tick_ms` while the directory holds records, from the lowest member
/// of this node's view only, so the log gets one a second and not three (D281, D289).
pub(super) fn spawn_ticks(ctx: &Arc<NodeContext>) -> JoinHandle<()> {
    let ctx = ctx.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(ctx.config.presence.tick());
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let lowest = ctx.group.view().members.first().copied();
            if lowest != Some(ctx.id)
                || !ctx.serving.load(Ordering::SeqCst)
                || ctx.degraded.is_active()
                || ctx.state.with(|s| s.directory().is_empty())
            {
                continue;
            }
            let at_ms = now_ms();
            if let Err(e) = ctx
                .group
                .send_ordered(Command::PresenceTick { at_ms })
                .await
            {
                tracing::debug!("presence tick not committed: {e}");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_user_has_one_proposer_and_users_spread_over_the_members() {
        let members = [1, 2, 3];
        let mut per_node = [0u32; 3];
        for client in 1..=10 {
            for n in 0..300 {
                let user = UserId { client, user: n };
                let chosen = proposer(&members, user).unwrap();
                assert_eq!(
                    proposer(&members, user),
                    Some(chosen),
                    "the same every time"
                );
                per_node[(chosen - 1) as usize] += 1;
            }
        }
        // 3,000 users: each node takes about a third.
        for count in per_node {
            assert!((800..1200).contains(&count), "{per_node:?}");
        }
    }

    #[test]
    fn with_one_member_gone_its_users_go_to_the_others() {
        let user = UserId { client: 4, user: 9 };
        let all = proposer(&[1, 2, 3], user).unwrap();
        let rest: Vec<u64> = [1, 2, 3].into_iter().filter(|n| *n != all).collect();
        assert!(rest.contains(&proposer(&rest, user).unwrap()));
        assert_eq!(proposer(&[], user), None);
    }

    #[test]
    fn outcomes_become_the_replies_the_client_expects() {
        assert_eq!(
            reply(PresenceOutcome::Renewed {
                lease_ms: 10_000,
                lease_ends_ms: 5,
                log_index: 7
            }),
            PresenceReply::Registered {
                lease_ms: 10_000,
                log_index: 7
            }
        );
        assert_eq!(
            reply(PresenceOutcome::Unknown { log_index: 1 }),
            PresenceReply::Unknown
        );
        assert!(matches!(
            reply(PresenceOutcome::Superseded {
                held: 9,
                log_index: 1
            }),
            PresenceReply::Rejected(reason) if reason.contains('9')
        ));
    }
}
