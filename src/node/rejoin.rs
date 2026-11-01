//! Rejoin and state recovery.
//!
//! A node that comes back from a fault catches up from the leader's log (Raft replication does
//! the copying) and serves nothing until it is current: no elections, no bids, no replies.
//! `NodeContext::serving` is the switch other parts check.
//!
//! The gate follows the fault gate (D75):
//!
//! - silence starts: `serving` goes false and elections are switched off, so the silent node
//!   does not raise its term and depose a healthy leader when it returns (D5);
//! - silence ends: log `rejoin_started`, wait for an AppendEntries from a current leader (which
//!   brings this node's term up to date), then ask that leader for its commit index with
//!   `PeerBody::ReadIndex`: the leader confirms with a majority that it still leads before it
//!   answers, so the target is never stale (D76). A node that still believes it leads (a short
//!   silence may not have cost it leadership) asks itself the same way;
//! - once the applied index reaches the target: `serving` true, elections back on, log
//!   `rejoined` with the recovery time (E4).
//!
//! A node that hears from no current leader keeps waiting and warns; it cannot know it is
//! current. Votes and log entries are accepted throughout, so the 2-of-3 majority stays intact.
//!
//! Owner: D (rejoin and recovery).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::metrics::Event;
use crate::node::NodeContext;

/// How often a node still catching up warns about it.
const WAIT_WARNING: Duration = Duration::from_secs(5);

/// A node is current once its applied index reaches the leader's commit index.
pub fn is_current(local_applied: Option<u64>, leader_commit: Option<u64>) -> bool {
    match leader_commit {
        // The leader has committed nothing yet, so there is nothing to catch up on.
        None => true,
        Some(commit) => local_applied.is_some_and(|applied| applied >= commit),
    }
}

/// Start watching for returns from silence.
pub fn spawn(ctx: &Arc<NodeContext>) -> Option<JoinHandle<()>> {
    let ctx = ctx.clone();
    // Runs for the node's lifetime; the node aborts it on shutdown.
    Some(tokio::spawn(async move {
        let mut gate = ctx.gate.subscribe();
        loop {
            if gate.wait_for(|silenced| *silenced).await.is_err() {
                return;
            }
            ctx.serving.store(false, Ordering::SeqCst);
            ctx.consensus.set_elect(false);
            tracing::info!(node = ctx.id, "not serving: silenced");

            if gate.wait_for(|silenced| !*silenced).await.is_err() {
                return;
            }
            catch_up(&ctx, &mut gate).await;
        }
    }))
}

/// Wait until this node is current, then serve again. Returns early, still not serving, if the
/// node is silenced again first.
async fn catch_up(ctx: &NodeContext, gate: &mut watch::Receiver<bool>) {
    let restored = Instant::now();
    // Only AppendEntries that arrive after the silence count.
    let mut commits = ctx.consensus.leader_commit();
    commits.mark_unchanged();
    ctx.log.log(Event::RejoinStarted {
        applied: ctx.consensus.status().last_applied,
    });

    let target = tokio::select! {
        target = learn_target(ctx, &mut commits) => target,
        _ = gate.wait_for(|silenced| *silenced) => return,
    };

    if let Some(index) = target {
        loop {
            tokio::select! {
                done = ctx.consensus.wait_applied(index, WAIT_WARNING) => {
                    if done {
                        break;
                    }
                    tracing::warn!(node = ctx.id, index, "rejoin: still applying the leader's log");
                }
                _ = gate.wait_for(|silenced| *silenced) => return,
            }
        }
    }

    let applied = ctx.consensus.status().last_applied;
    ctx.consensus.set_elect(true);
    ctx.serving.store(true, Ordering::SeqCst);
    let recovery_us = restored.elapsed().as_micros() as u64;
    ctx.log.log(Event::Rejoined {
        leader_commit: target,
        applied,
        recovery_us,
    });
    tracing::info!(node = ctx.id, ?target, recovery_us, "serving again");
}

/// The leader's commit index, confirmed by a majority (ReadIndex, D76).
///
/// A returning follower first waits for an AppendEntries from a current leader: until then its
/// term is old and the leader would drop its query (term rule, D21). A node that still believes
/// it leads asks itself; its check fails if it was replaced, and it waits for the new leader.
async fn learn_target(
    ctx: &NodeContext,
    commits: &mut watch::Receiver<Option<u64>>,
) -> Option<u64> {
    let retry = Duration::from_millis(ctx.config.timers.raft_heartbeat_ms);
    let rpc_timeout = ctx.config.timers.peer_rpc_timeout();
    let mut contacted = false;
    let mut last_warning = Instant::now();
    loop {
        if contacted || ctx.consensus.status().leader == Some(ctx.id) {
            if let Ok(Ok(target)) =
                tokio::time::timeout(rpc_timeout, ctx.consensus.read_index()).await
            {
                return target;
            }
        }
        if last_warning.elapsed() >= WAIT_WARNING {
            last_warning = Instant::now();
            tracing::warn!(
                node = ctx.id,
                contacted,
                "rejoin: no commit index from a leader yet"
            );
        }
        tokio::select! {
            changed = commits.changed() => {
                if changed.is_ok() {
                    contacted = true;
                } else {
                    // Consensus is shutting down; the node is going away.
                    std::future::pending::<()>().await;
                }
            }
            _ = tokio::time::sleep(retry) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_committed_means_current() {
        assert!(is_current(None, None));
        assert!(is_current(Some(4), None));
    }

    #[test]
    fn current_once_applied_reaches_the_leaders_commit() {
        assert!(!is_current(None, Some(0)));
        assert!(!is_current(Some(6), Some(7)));
        assert!(is_current(Some(7), Some(7)));
        assert!(is_current(Some(8), Some(7)));
    }
}
