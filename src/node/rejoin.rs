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
//! The answer is the larger of the leader's commit index and its first entry of the term, which is
//! not committed yet for a leader just elected. If that leader is replaced, the index may never
//! exist, so a wait that runs out asks the leader again and keeps the lowest answer (D135).
//!
//! A node restarted with a Raft log from disk (D118) goes through the same catch-up from the
//! start, elections off.
//!
//! Either way, a node that hears from no leader for [`NO_LEADER_ELECT_AFTER`] allows elections
//! again, and still serves nothing until it is current (D125). Without this, two nodes silenced
//! together hang the cluster: the third cannot win with a shorter log, and they stay out of the
//! running because nobody can tell them they are current.
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

/// How long a returning node waits for a leader before it may stand for election itself.
const NO_LEADER_ELECT_AFTER: Duration = Duration::from_secs(3);

/// How long a node waits on one target before it asks the leader again (D135).
const ASK_AGAIN_AFTER: Duration = Duration::from_secs(1);

/// A node is current once its applied index reaches the leader's commit index.
pub fn is_current(local_applied: Option<u64>, leader_commit: Option<u64>) -> bool {
    match leader_commit {
        // The leader has committed nothing yet, so there is nothing to catch up on.
        None => true,
        Some(commit) => local_applied.is_some_and(|applied| applied >= commit),
    }
}

/// Start watching for returns from silence.
pub fn spawn(ctx: &Arc<NodeContext>, restarted: bool) -> Option<JoinHandle<()>> {
    let ctx = ctx.clone();
    // Runs for the node's lifetime; the node aborts it on shutdown.
    Some(tokio::spawn(async move {
        let mut gate = ctx.gate.subscribe();
        if restarted {
            after_restart(&ctx, &mut gate).await;
        }
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
            catch_up_or_elect(&ctx, &mut gate).await;
        }
    }))
}

async fn after_restart(ctx: &NodeContext, gate: &mut watch::Receiver<bool>) {
    let status = ctx.consensus.status();
    tracing::info!(node = ctx.id, "restarted from the Raft log file");
    ctx.log.log(Event::Restarted {
        term: status.term,
        last_log_index: status.last_log_index,
    });
    catch_up_or_elect(ctx, gate).await;
}

/// Catch up, allowing elections if no leader shows up. A leader's heartbeats keep this node from
/// standing even with elections on, so a slow catch-up under a healthy leader deposes nobody.
async fn catch_up_or_elect(ctx: &NodeContext, gate: &mut watch::Receiver<bool>) {
    let allow_elections = async {
        tokio::time::sleep(NO_LEADER_ELECT_AFTER).await;
        tracing::warn!(node = ctx.id, "no leader yet: allowing elections");
        ctx.consensus.set_elect(true);
        std::future::pending::<()>().await
    };
    tokio::select! {
        () = catch_up(ctx, gate) => {}
        () = allow_elections => {}
    }
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
        target = reach_target(ctx, &mut commits) => target,
        _ = gate.wait_for(|silenced| *silenced) => return,
    };

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

/// Ask the leader for a target and wait until this node has applied it. Returns the target it
/// reached, or `None` if the leader has nothing committed.
///
/// A wait that runs out asks again and keeps the lowest answer. Every answer covers all that was
/// committed when it was given, so the lowest still covers everything committed before the first
/// question, and a commit index that rises under load cannot keep moving the finish line.
///
/// One question per wait: the leader confirms itself with a round of heartbeats for each one, and
/// retrying as `learn_target` does would add about 20 rounds a second to a cluster that is already
/// struggling. A question that fails changes nothing: the target it has may still be reached.
async fn reach_target(
    ctx: &NodeContext,
    commits: &mut watch::Receiver<Option<u64>>,
) -> Option<u64> {
    let rpc_timeout = ctx.config.timers.peer_rpc_timeout();
    let mut target = learn_target(ctx, commits).await?;
    let mut last_warning = Instant::now();
    loop {
        if ctx.consensus.wait_applied(target, ASK_AGAIN_AFTER).await {
            return Some(target);
        }
        if last_warning.elapsed() >= WAIT_WARNING {
            last_warning = Instant::now();
            tracing::warn!(
                node = ctx.id,
                target,
                "rejoin: still applying the leader's log"
            );
        }
        match tokio::time::timeout(rpc_timeout, ctx.consensus.read_index()).await {
            Ok(Ok(None)) => return None,
            Ok(Ok(Some(answer))) if answer < target => {
                tracing::warn!(node = ctx.id, target, answer, "rejoin: target lowered");
                ctx.log.log(Event::RejoinTargetLowered {
                    from: target,
                    to: answer,
                });
                target = answer;
            }
            _ => {}
        }
    }
}

/// One answer from the leader: its commit index, or the first entry of its term if that is higher,
/// confirmed by a majority (ReadIndex, D76).
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
    use crate::metrics::EventLog;
    use crate::node::testing::{ScriptedConsensus, context, log_dir, logged, until};
    use crate::proto::ProposeError;

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

    /// A node not serving yet, on a scripted consensus, with `catch_up` running.
    fn catching_up(
        fake: &Arc<ScriptedConsensus>,
        log: EventLog,
    ) -> (Arc<NodeContext>, JoinHandle<()>) {
        let ctx = context(fake.clone(), log, |_| {});
        ctx.serving.store(false, Ordering::SeqCst);
        let running = {
            let ctx = ctx.clone();
            tokio::spawn(async move {
                let mut gate = ctx.gate.subscribe();
                catch_up(&ctx, &mut gate).await;
            })
        };
        (ctx, running)
    }

    /// The rehearsal's stuck node: a leader just elected answers with its own first entry, is
    /// replaced, and that index never exists. The final log ends lower, and this node has all of it.
    #[tokio::test]
    async fn a_target_that_never_commits_does_not_hold_the_node_back_for_ever() {
        let dir = log_dir("rejoin-never-commits");
        let log = EventLog::to_dir(&dir, "node-1").unwrap();
        let fake = ScriptedConsensus::new(1);
        fake.set_leader(Some(2));
        fake.script_answers([Ok(Some(4004)), Ok(Some(3497))]);
        fake.apply_up_to(3497);
        let (ctx, running) = catching_up(&fake, log.clone());

        // One sign of a current leader, and then none: asking again must not depend on another.
        until("the first question", || {
            fake.hear_from_leader();
            fake.questions() >= 1
        })
        .await;
        tokio::time::timeout(Duration::from_secs(10), running)
            .await
            .expect("the node must not wait for ever")
            .unwrap();

        assert!(ctx.serving.load(Ordering::SeqCst));
        assert_eq!(fake.questions(), 2);
        let events = logged(&log, &dir);
        let event = |name: &str| events.iter().find(|e| e["ev"] == name).unwrap().clone();
        assert_eq!(event("rejoin_target_lowered")["from"], 4004);
        assert_eq!(event("rejoin_target_lowered")["to"], 3497);
        assert_eq!(event("rejoined")["leader_commit"], 3497);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn asking_again_does_not_raise_the_target() {
        let dir = log_dir("rejoin-not-raised");
        let log = EventLog::to_dir(&dir, "node-1").unwrap();
        let fake = ScriptedConsensus::new(1);
        // Under load every answer is higher than the last.
        fake.set_leader(Some(1));
        fake.script_answers([Ok(Some(10)), Ok(Some(20)), Ok(Some(30))]);
        fake.apply_up_to(5);
        let (ctx, running) = catching_up(&fake, log.clone());

        until("a second question", || fake.questions() >= 2).await;
        assert!(!ctx.serving.load(Ordering::SeqCst), "applied is only 5");
        fake.apply_up_to(10);
        running.await.unwrap();

        assert!(ctx.serving.load(Ordering::SeqCst));
        let events = logged(&log, &dir);
        let rejoined = events.iter().find(|e| e["ev"] == "rejoined").unwrap();
        assert_eq!(rejoined["leader_commit"], 10);
        assert!(!events.iter().any(|e| e["ev"] == "rejoin_target_lowered"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The leader that gave the target goes away just as the wait runs out. The target it gave is
    /// still good, so the node must not need a new leader to finish.
    #[tokio::test]
    async fn with_no_leader_to_ask_the_old_target_still_counts() {
        let fake = ScriptedConsensus::new(1);
        fake.set_leader(Some(1));
        fake.script_answers([Ok(Some(10)), Err(ProposeError::Timeout)]);
        let (ctx, running) = catching_up(&fake, EventLog::disabled("node-1"));

        until("a second question", || fake.questions() >= 2).await;
        fake.apply_up_to(10);
        running.await.unwrap();

        assert!(ctx.serving.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_leader_with_nothing_committed_needs_no_wait() {
        let fake = ScriptedConsensus::new(1);
        fake.set_leader(Some(1));
        fake.script_answers([Ok(None)]);
        let (ctx, running) = catching_up(&fake, EventLog::disabled("node-1"));

        running.await.unwrap();

        assert!(ctx.serving.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn silenced_again_while_waiting_it_stays_not_serving() {
        let fake = ScriptedConsensus::new(1);
        fake.set_leader(Some(1));
        fake.script_answers([Ok(Some(10))]);
        let (ctx, running) = catching_up(&fake, EventLog::disabled("node-1"));

        until("the first question", || fake.questions() >= 1).await;
        ctx.gate.silence();
        running.await.unwrap();

        assert!(!ctx.serving.load(Ordering::SeqCst));
    }
}
