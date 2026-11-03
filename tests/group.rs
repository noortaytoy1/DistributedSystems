//! Part C: the cloud group on three real nodes: who is listed, who a send reaches, and how a node
//! gets back on the list (D151-D155). Never asserts a view number: it means nothing across nodes.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{TestCluster, first_reply, wait_until};
use rescuemesh::client::control;
use rescuemesh::config::{AssignmentPolicy, Config};
use rescuemesh::metrics::Event;
use rescuemesh::node::transport::TransportError;
use rescuemesh::proto::{
    Bid, Capability, ClientRequest, ControlReply, ControlRequest, Incident, NodeId, NodeStatus,
    PeerBody, PeerReply, Point, RequestId,
};
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(10);

/// Short, so a test does not wait out the real 2 s.
const HOLD: Duration = Duration::from_millis(300);

fn short_hold(c: &mut Config) {
    c.timers.degraded_after_ms = HOLD.as_millis() as u64;
}

fn members(cluster: &TestCluster, id: NodeId) -> Vec<NodeId> {
    cluster
        .node(id)
        .ctx
        .group
        .view()
        .members
        .into_iter()
        .collect()
}

fn addressed<T>(answers: &[(NodeId, T)]) -> Vec<NodeId> {
    answers.iter().map(|(peer, _)| *peer).collect()
}

/// A leader and term every node agrees on, so a peer's `StaleTerm` cannot be mistaken for a fault.
async fn agreed_leader(cluster: &TestCluster) {
    wait_until(WAIT, "a leader and term every node agrees on", || {
        let status: Vec<_> = cluster
            .nodes
            .iter()
            .map(|n| n.ctx.consensus.status())
            .collect();
        status[0].leader.is_some()
            && status
                .iter()
                .all(|s| s.leader == status[0].leader && s.term == status[0].term)
    })
    .await;
}

fn log_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rescuemesh-group-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// One node's events, in the order it logged them. Read after the cluster has shut down.
fn events_of(dir: &Path, src: &str) -> Vec<serde_json::Value> {
    std::fs::read_to_string(dir.join(format!("{src}.jsonl")))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn request(seq: u64) -> ClientRequest {
    ClientRequest {
        id: RequestId { client: 88, seq },
        attempt: 0,
        incident: Incident {
            zone: 0,
            location: Point { x: 10, y: 20 },
            severity: 1,
            needs: Capability::Medical,
            reported_at_ms: 0,
        },
    }
}

fn bids_sent(cluster: &TestCluster, id: NodeId) -> u64 {
    match cluster.node(id).ctx.log.msgs().snapshot() {
        Event::Messages { sent, .. } => sent["bid"],
        _ => 0,
    }
}

fn bids_seen(cluster: &TestCluster, id: NodeId, key: (RequestId, u32)) -> usize {
    cluster
        .node(id)
        .ctx
        .bids
        .with_rounds(|rounds| rounds.bids_seen(key))
}

/// Sends one request to every node, waits for node 1's round on it to close, and returns how many
/// copies of its bid node 1 sent.
async fn close_a_round(cluster: &TestCluster, seq: u64) -> ((RequestId, u32), u64) {
    let before = bids_sent(cluster, 1);
    let req = request(seq);
    let key = (req.id, req.attempt);
    // The round is what matters here; the reply only says the request got through.
    first_reply(&cluster.config, req, WAIT).await;
    wait_until(WAIT, "node 1's round to close", || {
        let ranked = cluster.node(1).ctx.bids.with_rounds(|r| r.ranked_bids(key));
        ranked.is_some() && bids_sent(cluster, 1) > before
    })
    .await;
    (key, bids_sent(cluster, 1) - before)
}

async fn status_of(cluster: &TestCluster, id: NodeId) -> NodeStatus {
    let addr = &cluster.config.node(id).unwrap().client_addr;
    let reply = control(addr, ControlRequest::Status, Duration::from_secs(2));
    match reply.await.unwrap() {
        ControlReply::Status(status) => status,
        other => panic!("unexpected reply {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silenced_member_leaves_after_the_hold_and_returns_when_heard() {
    let dir = log_dir("leaves");
    let metrics = dir.clone();
    let cluster = TestCluster::start_with(3, move |c| {
        short_hold(c);
        c.metrics.dir = Some(metrics);
    })
    .await;
    for id in 1..=3 {
        assert_eq!(members(&cluster, id), [1, 2, 3]);
    }

    cluster.node(3).ctx.gate.silence();
    let silenced_at = Instant::now();
    wait_until(WAIT, "nodes 1 and 2 to stop listing node 3", || {
        members(&cluster, 1) == [1, 2] && members(&cluster, 2) == [1, 2]
    })
    .await;
    // Being suspected is not enough: the whole hold has to pass first.
    assert!(silenced_at.elapsed() >= HOLD);
    // The silenced node is the one cut off, so its own list does not move.
    assert_eq!(members(&cluster, 3), [1, 2, 3]);

    cluster.node(3).ctx.gate.restore();
    wait_until(WAIT, "everyone to list all three again", || {
        (1..=3).all(|id| members(&cluster, id) == [1, 2, 3])
    })
    .await;
    cluster.shutdown().await;

    let events = events_of(&dir, "node-1");
    let first = |name: &str| events.iter().position(|e| e["ev"] == name).unwrap();
    assert!(first("started") < first("group_view"), "{events:?}");
    let views: Vec<Vec<NodeId>> = events
        .iter()
        .filter(|e| e["ev"] == "group_view")
        .map(|e| serde_json::from_value(e["members"].clone()).unwrap())
        .collect();
    assert_eq!(views[0], [1, 2, 3], "{views:?}");
    assert!(views.contains(&vec![1, 2]), "{views:?}");
    assert_eq!(views[views.len() - 1], [1, 2, 3], "{views:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn send_reports_who_answered_and_skips_a_member_who_left() {
    // A longer hold: the send right after the silence must still find node 3 on the list.
    let cluster = TestCluster::start_with(3, |c| {
        c.policy.assignment = AssignmentPolicy::Bidding;
        c.timers.degraded_after_ms = 1_000;
    })
    .await;
    agreed_leader(&cluster).await;
    let group = &cluster.node(1).ctx.group;
    let bid = Bid {
        request: RequestId { client: 99, seq: 1 },
        attempt: 0,
        score: 1,
    };
    let send = || group.send(Duration::from_millis(200), || PeerBody::Bid(bid.clone()));

    // Both peers take it. Polled, because an election on a busy machine can still move the term,
    // and a peer on a newer term answers StaleTerm.
    let until = Instant::now() + WAIT;
    loop {
        let everyone = send().await;
        assert_eq!(addressed(&everyone), [2, 3]);
        let taken = |(_, answer): &(NodeId, Result<PeerReply, TransportError>)| {
            matches!(answer, Ok(PeerReply::Ack))
        };
        if everyone.iter().all(taken) {
            break;
        }
        assert!(Instant::now() < until, "{everyone:?}");
    }

    // Inside the hold node 3 is still a member, so it is addressed and reported as missed.
    cluster.node(3).ctx.gate.silence();
    let missed = send().await;
    assert_eq!(addressed(&missed), [2, 3]);
    // Not Ack: if node 3 led, an election may have moved the term, and a peer answers StaleTerm.
    assert!(missed[0].1.is_ok(), "{missed:?}");
    assert!(
        matches!(missed[1].1, Err(TransportError::Timeout(_))),
        "{missed:?}"
    );

    wait_until(WAIT, "node 1 to stop listing node 3", || {
        members(&cluster, 1) == [1, 2]
    })
    .await;
    let after = send().await;
    assert_eq!(addressed(&after), [2]);
    assert!(after[0].1.is_ok(), "{after:?}");

    cluster.node(3).ctx.gate.restore();
    cluster.shutdown().await;
}

/// The trap that makes a probe different from a send: if heartbeats followed the list, two live
/// nodes that removed each other would never speak again, and a 20 s fault would last for ever.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_still_reaches_a_peer_outside_the_view() {
    let cluster = TestCluster::start_with(3, short_hold).await;

    cluster.node(3).ctx.gate.silence();
    wait_until(WAIT, "node 1 to stop listing node 3", || {
        members(&cluster, 1) == [1, 2]
    })
    .await;
    let probe = cluster
        .node(1)
        .ctx
        .group
        .probe(Duration::from_millis(100), || PeerBody::Heartbeat)
        .await;
    assert_eq!(addressed(&probe), [2, 3]);
    assert!(
        matches!(probe[1].1, Err(TransportError::Timeout(_))),
        "{probe:?}"
    );

    // Swap who is silent. Node 3 now drops node 1, while node 1's list stays frozen without node
    // 3, so these two have removed each other.
    cluster.node(1).ctx.gate.silence();
    cluster.node(3).ctx.gate.restore();
    wait_until(WAIT, "node 3 to stop listing node 1", || {
        members(&cluster, 3) == [2, 3]
    })
    .await;
    assert_eq!(members(&cluster, 1), [1, 2]);

    cluster.node(1).ctx.gate.restore();
    wait_until(WAIT, "nodes 1 and 3 to find each other again", || {
        members(&cluster, 1) == [1, 2, 3] && members(&cluster, 3) == [1, 2, 3]
    })
    .await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bids_reach_a_returned_node_again() {
    let cluster = TestCluster::start_with(3, |c| {
        short_hold(c);
        c.policy.assignment = AssignmentPolicy::Bidding;
        // Wide, so a busy machine still gets every bid in before a round closes.
        c.timers.bid_window_ms = 100;
    })
    .await;
    agreed_leader(&cluster).await;

    let (_, copies) = close_a_round(&cluster, 0).await;
    assert_eq!(copies, 2, "everyone is listed");

    cluster.node(3).ctx.gate.silence();
    wait_until(WAIT, "node 1 to stop listing node 3", || {
        members(&cluster, 1) == [1, 2]
    })
    .await;
    let (_, copies) = close_a_round(&cluster, 1).await;
    assert_eq!(copies, 1, "a member who left gets no copy");

    cluster.node(3).ctx.gate.restore();
    wait_until(
        WAIT,
        "everyone listing all three, and node 3 serving",
        || {
            (1..=3).all(|id| members(&cluster, id) == [1, 2, 3])
                && cluster.node(3).ctx.status().serving
        },
    )
    .await;

    // Node 3 hears from both peers again, in the same round, and node 1 sends two copies again.
    // More than one round may pass while the leader settles after the fault.
    let until = Instant::now() + WAIT;
    for seq in 2.. {
        let (key, copies) = close_a_round(&cluster, seq).await;
        assert_eq!(copies, 2, "node 3 is listed again");
        if bids_seen(&cluster, 3, key) == 3 {
            break;
        }
        assert!(Instant::now() < until, "node 3 never saw a round of 3 bids");
    }
    cluster.shutdown().await;
}

/// Demo scene 6: node 1 loses power, then node 2 goes silent and comes back. Node 2 must not list
/// the dead node again just because it has been away itself (D151).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_peer_suspected_before_a_silence_stays_out_after_restore() {
    let cluster = TestCluster::start_with(3, short_hold).await;

    cluster.node(1).ctx.gate.silence();
    wait_until(WAIT, "node 2 to stop listing node 1", || {
        members(&cluster, 2) == [2, 3]
    })
    .await;

    cluster.node(2).ctx.gate.silence();
    wait_until(WAIT, "node 3 to notice node 2", || {
        let suspicion = cluster.node(3).ctx.suspicion.lock().unwrap();
        suspicion.is_suspected(2)
    })
    .await;
    cluster.node(2).ctx.gate.restore();

    // Long past the point where a fresh clock would have listed node 1 again.
    let until = Instant::now() + 2 * HOLD;
    while Instant::now() < until {
        assert_eq!(members(&cluster, 2), [2, 3]);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    cluster.node(1).ctx.gate.restore();
    wait_until(WAIT, "node 2 to list node 1 again", || {
        members(&cluster, 2) == [1, 2, 3]
    })
    .await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_reports_members() {
    let cluster = TestCluster::start_with(3, short_hold).await;
    assert_eq!(status_of(&cluster, 1).await.members, [1, 2, 3]);

    cluster.node(3).ctx.gate.silence();
    wait_until(WAIT, "node 1 to stop listing node 3", || {
        members(&cluster, 1) == [1, 2]
    })
    .await;
    assert_eq!(status_of(&cluster, 1).await.members, [1, 2]);

    // A silenced node still answers control frames. Its list is frozen, which `ctl` prints as `-`.
    let silent = status_of(&cluster, 3).await;
    assert!(silent.silenced);
    assert_eq!(silent.members, [1, 2, 3]);

    cluster.node(3).ctx.gate.restore();
    cluster.shutdown().await;
}
