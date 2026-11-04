//! Rotating rounds: one order with no leader, on real nodes in one process (D249).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::config::{Config, ConsensusMode};
use rescuemesh::proto::{
    Capability, Category, Command, Incident, NodeId, Outcome, PeerReply, Plan, Point, RequestId,
    Responder, ResponderId, ResultProposal, RoundsMsg, RoundsReply, UserId,
};
use rescuemesh::raft::{Consensus, Role};

const WAIT: Duration = Duration::from_secs(20);

fn rounds(config: &mut Config) {
    config.consensus = ConsensusMode::Rounds;
    config.faults.enabled = false;
}

fn proposal(seq: u64, executor: NodeId, responder: Option<ResponderId>) -> Command {
    Command::CommitResult(ResultProposal {
        request: RequestId { client: 1, seq },
        incident: Incident {
            owner: UserId { client: 1, user: 0 },
            category: Category::Flooding,
            zone: 0,
            location: Point { x: 5, y: 5 },
            severity: 2,
            needs: Capability::Rescue,
            reported_at_ms: 0,
            ttl_ms: 60_000,
        },
        executor,
        term: 0,
        plan: Plan {
            responder,
            cost: 1,
            priority: 1,
        },
        age_ms: 0,
        assigned_at_ms: 0,
    })
}

fn register(ids: impl IntoIterator<Item = ResponderId>) -> Command {
    Command::RegisterResponders {
        responders: ids
            .into_iter()
            .map(|id| Responder {
                id,
                zone: 0,
                location: Point { x: 0, y: 0 },
                capability: Capability::Rescue,
            })
            .collect(),
    }
}

fn consensus(cluster: &TestCluster, id: NodeId) -> Arc<dyn Consensus> {
    cluster.node(id).ctx.consensus.clone()
}

async fn same_state(cluster: &TestCluster, ids: &[NodeId], results: usize) {
    wait_until(WAIT, "the nodes to hold one state", || {
        let views: Vec<_> = ids
            .iter()
            .map(|id| {
                cluster
                    .node(*id)
                    .ctx
                    .state
                    .with(|s| (s.result_count(), s.digest()))
            })
            .collect();
        views[0].0 >= results && views.iter().all(|v| *v == views[0])
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nobody_leads_and_every_node_commits_to_one_order() {
    let cluster = TestCluster::start_with(3, rounds).await;
    for node in &cluster.nodes {
        let status = node.ctx.consensus.status();
        assert_eq!(status.leader, None);
        assert_eq!(status.role, Role::Peer);
    }
    consensus(&cluster, 1)
        .propose(register(1..=500))
        .await
        .unwrap();
    let mut proposals = Vec::new();
    for seq in 0..300u64 {
        let node = (seq % 3) + 1;
        let c = consensus(&cluster, node);
        proposals.push(tokio::spawn(async move {
            c.propose(proposal(seq, node, Some(seq as u32 + 1))).await
        }));
    }
    for p in proposals {
        assert!(matches!(p.await.unwrap(), Ok(Outcome::Committed(_))));
    }
    same_state(&cluster, &[1, 2, 3], 300).await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_first_in_the_order_wins_on_every_node() {
    let cluster = TestCluster::start_with(3, rounds).await;
    consensus(&cluster, 2)
        .propose(register([1, 2]))
        .await
        .unwrap();
    let (a, b) = (consensus(&cluster, 1), consensus(&cluster, 3));
    let (first, second) = tokio::join!(
        a.propose(proposal(7, 1, Some(1))),
        b.propose(proposal(7, 3, Some(2))),
    );
    let winner = |outcome: Outcome| match outcome {
        Outcome::Committed(r) | Outcome::Duplicate(r) => r.executor,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(winner(first.unwrap()), winner(second.unwrap()));
    same_state(&cluster, &[1, 2, 3], 1).await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_others_go_on_without_a_silent_node_and_it_catches_up_after() {
    let cluster = TestCluster::start_with(3, rounds).await;
    let c1 = consensus(&cluster, 1);
    c1.propose(register(1..=500)).await.unwrap();
    c1.propose(proposal(1, 1, Some(1))).await.unwrap();
    same_state(&cluster, &[1, 2, 3], 1).await;

    cluster.node(3).ctx.gate.silence();
    // Blocked until the two agree to go on, then it commits.
    for seq in 2..20u64 {
        c1.propose(proposal(seq, 1, Some(seq as u32)))
            .await
            .unwrap();
    }
    same_state(&cluster, &[1, 2], 19).await;
    assert!(
        cluster.node(1).ctx.consensus.status().term > 0,
        "a new view"
    );

    cluster.node(3).ctx.gate.restore();
    same_state(&cluster, &[1, 2, 3], 19).await;
    wait_until(WAIT, "node 3 to serve again", || {
        cluster
            .node(3)
            .ctx
            .serving
            .load(std::sync::atomic::Ordering::SeqCst)
    })
    .await;

    // Back as a full member: its own proposals commit, and all three agree.
    let c3 = consensus(&cluster, 3);
    let until = tokio::time::Instant::now() + WAIT;
    loop {
        // An attempt that ran out of time can still commit, so the next says it is already decided.
        if matches!(
            c3.propose(proposal(50, 3, Some(50))).await,
            Ok(Outcome::Committed(_) | Outcome::Duplicate(_))
        ) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "node 3 never came back"
        );
    }
    same_state(&cluster, &[1, 2, 3], 20).await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_left_alone_stops_committing_until_a_peer_returns() {
    let cluster = TestCluster::start_with(3, |c| {
        rounds(c);
        c.timers.propose_timeout_ms = 500;
    })
    .await;
    let c1 = consensus(&cluster, 1);
    c1.propose(register(1..=10)).await.unwrap();
    cluster.node(2).ctx.gate.silence();
    cluster.node(3).ctx.gate.silence();
    assert!(c1.propose(proposal(1, 1, Some(1))).await.is_err());
    let before = cluster.node(1).ctx.state.with(|s| s.result_count());
    assert_eq!(before, 0, "one node alone must not commit");

    cluster.node(2).ctx.gate.restore();
    cluster.node(3).ctx.gate.restore();
    let until = tokio::time::Instant::now() + WAIT;
    while c1.propose(proposal(2, 1, Some(2))).await.is_err() {
        assert!(tokio::time::Instant::now() < until, "rounds never resumed");
    }
    same_state(&cluster, &[1, 2, 3], 1).await;
    cluster.shutdown().await;
}

/// Commands keep flowing from every node while one node after another goes silent and returns. Any
/// node that applied past the agreed end of its stream would end with a different state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn digests_stay_equal_through_repeated_drops_under_load() {
    let cluster = TestCluster::start_with(3, rounds).await;
    consensus(&cluster, 1)
        .propose(register(1..=50))
        .await
        .unwrap();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut load = Vec::new();
    for node in 1..=3u64 {
        for worker in 0..8u64 {
            let (c, stop) = (consensus(&cluster, node), stop.clone());
            load.push(tokio::spawn(async move {
                let mut seq = node * 1_000_000 + worker * 10_000;
                while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                    let responder = (seq % 50) as u32 + 1;
                    let _ = c.propose(proposal(seq, node, Some(responder))).await;
                    seq += 1;
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            }));
        }
    }
    for victim in [3u64, 1, 2, 3, 1] {
        tokio::time::sleep(Duration::from_millis(400)).await;
        cluster.node(victim).ctx.gate.silence();
        tokio::time::sleep(Duration::from_millis(500)).await;
        cluster.node(victim).ctx.gate.restore();
        wait_until(WAIT, "all three members again", || {
            cluster
                .nodes
                .iter()
                .all(|n| n.ctx.serving.load(std::sync::atomic::Ordering::SeqCst))
        })
        .await;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    for task in load {
        let _ = task.await;
    }
    same_state(&cluster, &[1, 2, 3], 1).await;
    cluster.shutdown().await;
}

/// A node asked to commit while silenced makes batches nobody else will ever hold. Once back, none of
/// them may reach the others, or their logs would hold commands it never applied.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn what_a_silent_node_was_asked_never_reaches_the_others() {
    let cluster = TestCluster::start_with(3, |c| {
        rounds(c);
        c.timers.propose_timeout_ms = 300;
    })
    .await;
    let c1 = consensus(&cluster, 1);
    c1.propose(register(1..=50)).await.unwrap();
    c1.propose(proposal(1, 1, Some(1))).await.unwrap();
    same_state(&cluster, &[1, 2, 3], 1).await;

    cluster.node(3).ctx.gate.silence();
    let c3 = consensus(&cluster, 3);
    for seq in 100..110u64 {
        let _ = c3.propose(proposal(seq, 3, Some(seq as u32 - 90))).await;
    }
    cluster.node(3).ctx.gate.restore();
    wait_until(WAIT, "node 3 to serve again", || {
        cluster
            .node(3)
            .ctx
            .serving
            .load(std::sync::atomic::Ordering::SeqCst)
    })
    .await;
    let until = tokio::time::Instant::now() + WAIT;
    while !matches!(
        c3.propose(proposal(200, 3, Some(30))).await,
        Ok(Outcome::Committed(_) | Outcome::Duplicate(_))
    ) {
        assert!(
            tokio::time::Instant::now() < until,
            "node 3 never came back"
        );
    }
    same_state(&cluster, &[1, 2, 3], 2).await;
    for seq in 100..110u64 {
        let id = RequestId { client: 1, seq };
        assert!(
            cluster.node(1).ctx.state.with(|s| s.result(&id).is_none()),
            "a command from the silence reached node 1"
        );
    }
    cluster.shutdown().await;
}

async fn view_reached(cluster: &TestCluster, ids: &[NodeId], view: u64) {
    wait_until(WAIT, "the nodes to reach the view", || {
        ids.iter()
            .all(|id| cluster.node(*id).ctx.consensus.status().term >= view)
    })
    .await;
}

async fn commit_through(cluster: &TestCluster, node: NodeId, seq: u64) {
    let c = consensus(cluster, node);
    let until = tokio::time::Instant::now() + WAIT;
    while !matches!(
        c.propose(proposal(seq, node, Some(seq as u32))).await,
        Ok(Outcome::Committed(_) | Outcome::Duplicate(_))
    ) {
        assert!(
            tokio::time::Instant::now() < until,
            "node {node} never committed"
        );
    }
}

/// A promise given to a peer holds even when this node's own call to that peer fails. On the lab PCs
/// it was freed, so one node went on without node 1 and the other two without node 2 (D252).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_given_holds_when_the_asker_goes_quiet() {
    let cluster = TestCluster::start_with(3, rounds).await;
    consensus(&cluster, 1)
        .propose(register(1..=50))
        .await
        .unwrap();
    commit_through(&cluster, 1, 1).await;
    same_state(&cluster, &[1, 2, 3], 1).await;

    cluster.node(2).ctx.gate.silence();
    let c3 = consensus(&cluster, 3);
    let drop = |target| RoundsMsg::Drop {
        view: 1,
        target,
        have: 0,
    };
    let first = c3.handle_rounds(2, drop(1)).await;
    assert!(matches!(
        first,
        PeerReply::Rounds(RoundsReply::Granted { .. })
    ));
    // Node 3 now ignores node 1, finds it silent, and its own call to node 2 fails.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let second = c3.handle_rounds(1, drop(2)).await;
    assert!(
        matches!(second, PeerReply::Rounds(RoundsReply::Refused { .. })),
        "the same view was promised for a second drop: {second:?}"
    );

    // The asker returns, the agreed drop goes through, and all three end up the same.
    cluster.node(2).ctx.gate.restore();
    commit_through(&cluster, 2, 2).await;
    commit_through(&cluster, 3, 3).await;
    commit_through(&cluster, 1, 4).await;
    same_state(&cluster, &[1, 2, 3], 4).await;
    cluster.shutdown().await;
}

/// A node on its way back asks again while it has not seen its entry in the log. That must not count
/// against it: on the lab PCs it got the node dropped within seconds of being added (D252).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn asking_again_does_not_drop_a_node_on_its_way_back() {
    let cluster = TestCluster::start_with(3, rounds).await;
    let c1 = consensus(&cluster, 1);
    c1.propose(register(1..=50)).await.unwrap();
    cluster.node(3).ctx.gate.silence();
    view_reached(&cluster, &[1, 2], 1).await;
    let _ = c1.handle_rounds(3, RoundsMsg::Join).await;
    view_reached(&cluster, &[1, 2], 2).await;
    let _ = c1.handle_rounds(3, RoundsMsg::Join).await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(c1.status().term, 2, "the node was dropped for asking again");
    cluster.node(3).ctx.gate.restore();
    commit_through(&cluster, 3, 1).await;
    same_state(&cluster, &[1, 2, 3], 1).await;
    cluster.shutdown().await;
}

/// Added, dropped before getting back, added again: it must come back at the later round. On the lab
/// PCs it used the first, was then sent batches it had already applied, and its digest split (D252).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_added_twice_comes_back_at_the_later_round() {
    let cluster = TestCluster::start_with(3, rounds).await;
    let c1 = consensus(&cluster, 1);
    c1.propose(register(1..=50)).await.unwrap();
    commit_through(&cluster, 1, 1).await;
    same_state(&cluster, &[1, 2, 3], 1).await;

    cluster.node(3).ctx.gate.silence();
    view_reached(&cluster, &[1, 2], 1).await;
    commit_through(&cluster, 1, 2).await;
    let _ = c1.handle_rounds(3, RoundsMsg::Join).await;
    view_reached(&cluster, &[1, 2], 2).await;
    // It does not get back in time, so it is dropped again, and more is applied without it.
    view_reached(&cluster, &[1, 2], 3).await;
    commit_through(&cluster, 2, 3).await;
    commit_through(&cluster, 1, 4).await;
    let _ = c1.handle_rounds(3, RoundsMsg::Join).await;
    view_reached(&cluster, &[1, 2], 4).await;
    cluster.node(3).ctx.gate.restore();

    commit_through(&cluster, 3, 5).await;
    commit_through(&cluster, 1, 6).await;
    commit_through(&cluster, 2, 7).await;
    same_state(&cluster, &[1, 2, 3], 7).await;
    cluster.shutdown().await;
}
