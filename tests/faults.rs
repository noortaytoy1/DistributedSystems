//! Part C: the leader's fault schedule on three real nodes (D12, D78).
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::config::Config;
use rescuemesh::proto::{PeerBody, PeerEnvelope, PeerReply};
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(10);

/// Short timings so a test sees several rounds in a few seconds. The shortest wait is no shorter
/// than the longest silence, as validation requires (D78).
fn fast_faults(c: &mut Config) {
    c.faults.interval_min_ms = 300;
    c.faults.interval_max_ms = 400;
    c.faults.silence_min_ms = 100;
    c.faults.silence_max_ms = 300;
    c.faults.seed = Some(7);
}

fn silenced_count(cluster: &TestCluster) -> usize {
    (1..=3)
        .filter(|id| cluster.node(*id).ctx.gate.is_silenced())
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_schedule_injects_faults_one_victim_at_a_time() {
    let cluster = TestCluster::start_with(3, fast_faults).await;
    let deadline = Instant::now() + WAIT;
    let mut faults = 0;
    let mut was_silenced = [false; 3];
    while faults < 3 {
        assert!(
            Instant::now() < deadline,
            "only {faults} faults in {WAIT:?}"
        );
        assert!(silenced_count(&cluster) <= 1, "two nodes silenced at once");
        for id in 1..=3u64 {
            let now = cluster.node(id).ctx.gate.is_silenced();
            let before = &mut was_silenced[(id - 1) as usize];
            if now && !*before {
                faults += 1;
            }
            *before = now;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_order_from_an_old_term_is_dropped() {
    let cluster = TestCluster::start(3).await;
    wait_until(WAIT, "a leader in a term above 0", || {
        (1..=3).all(|id| {
            let s = cluster.node(id).ctx.consensus.status();
            s.leader.is_some() && s.term > 0
        })
    })
    .await;

    let stale = PeerEnvelope {
        from: 1,
        term: 0,
        body: PeerBody::FaultOrder { ms: 1_000 },
    };
    let peer = cluster.node(1).ctx.peers.get(2).unwrap().clone();
    let reply = peer.call(stale, Duration::from_secs(2)).await.unwrap();
    assert!(matches!(reply, PeerReply::StaleTerm { .. }), "{reply:?}");
    assert!(!cluster.node(2).ctx.gate.is_silenced());
    cluster.shutdown().await;
}

/// Obeying an order closes the victim's gate; its "ok" must still reach the leader, or every
/// obeyed order looks like an unreachable victim (D78).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_obeyed_order_is_acknowledged() {
    let cluster = TestCluster::start(3).await;
    wait_until(WAIT, "a leader", || {
        (1..=3).all(|id| cluster.node(id).ctx.consensus.status().leader.is_some())
    })
    .await;

    let term = cluster.node(1).ctx.consensus.status().term;
    let order = PeerEnvelope {
        from: 1,
        term,
        body: PeerBody::FaultOrder { ms: 300 },
    };
    let peer = cluster.node(1).ctx.peers.get(2).unwrap().clone();
    let reply = peer.call(order, Duration::from_secs(2)).await.unwrap();
    assert!(matches!(reply, PeerReply::FaultOrder(Ok(()))), "{reply:?}");
    assert!(cluster.node(2).ctx.gate.is_silenced());
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_scheduled_faults_when_fault_injection_is_disabled() {
    let cluster = TestCluster::start_with(3, |c| {
        fast_faults(c);
        c.faults.enabled = false;
    })
    .await;
    // Several intervals' worth of time with nobody ever silenced.
    let until = Instant::now() + Duration::from_millis(1_500);
    while Instant::now() < until {
        assert_eq!(silenced_count(&cluster), 0);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    cluster.shutdown().await;
}
