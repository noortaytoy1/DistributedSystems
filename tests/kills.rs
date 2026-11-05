//! Killing one, two and all three nodes, in every order: what is left keeps answering, and the
//! nodes come back by themselves when they restart (D261).
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use common::{TestCluster, first_reply, wait_until};
use rescuemesh::config::{AssignmentPolicy, Config, ConsensusMode};
use rescuemesh::node::transport::ClientPort;
use rescuemesh::node::{self, NodeHandle};
use rescuemesh::proto::{
    Capability, Category, ClientRequest, Incident, Point, ReplyBody, RequestId, UserId,
};
use tokio::net::TcpListener;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(30);

fn request(seq: u64) -> ClientRequest {
    ClientRequest {
        id: RequestId { client: 77, seq },
        attempt: 0,
        age_ms: 0,
        incident: Incident {
            owner: UserId {
                client: 77,
                user: 0,
            },
            category: Category::MedicalAssistance,
            zone: 0,
            location: Point { x: 1, y: 2 },
            severity: 3,
            needs: Capability::Medical,
            reported_at_ms: 0,
            ttl_ms: 600_000,
        },
    }
}

struct Lab {
    config: Config,
    nodes: BTreeMap<u64, NodeHandle>,
    dir: PathBuf,
    next: u64,
}

impl Lab {
    async fn start(name: &str) -> Lab {
        let dir = std::env::temp_dir().join(format!("rescuemesh-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let storage = dir.clone();
        let cluster = TestCluster::start_with(3, move |c| {
            c.consensus = ConsensusMode::Rounds;
            c.faults.enabled = false;
            c.policy.assignment = AssignmentPolicy::Bidding;
            c.storage.dir = Some(storage);
            c.timers.degraded_after_ms = 300;
        })
        .await;
        let config = cluster.config.clone();
        let nodes = cluster
            .nodes
            .into_iter()
            .enumerate()
            .map(|(at, node)| (at as u64 + 1, node))
            .collect();
        Lab {
            config,
            nodes,
            dir,
            next: 1,
        }
    }

    async fn kill(&mut self, id: u64) {
        self.nodes.remove(&id).unwrap().shutdown().await;
    }

    async fn bring_back(&mut self, id: u64) {
        let me = self.config.node(id).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let peer = loop {
            match TcpListener::bind(&me.peer_addr).await {
                Ok(listener) => break listener,
                Err(e) if Instant::now() > deadline => panic!("{e}"),
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        };
        let client = loop {
            match ClientPort::bind(&me.client_addr).await {
                Ok(port) => break port,
                Err(e) if Instant::now() > deadline => panic!("{e}"),
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        };
        let node = node::start_with(self.config.clone(), id, peer, client)
            .await
            .unwrap();
        self.nodes.insert(id, node);
    }

    /// A new report must end up committed, as a client retrying would see it.
    async fn commits(&mut self, what: &str) -> u64 {
        let seq = self.next;
        self.next += 1;
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            let reply = first_reply(&self.config, request(seq), Duration::from_secs(2)).await;
            if matches!(reply.map(|reply| reply.body), Some(ReplyBody::Done(_))) {
                return seq;
            }
        }
        panic!("{what}: nothing committed in {WAIT:?}");
    }

    /// A new report must get an answer, labelled provisional, from a node left alone.
    async fn answers_alone(&mut self, what: &str) {
        let seq = self.next;
        self.next += 1;
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            let reply = first_reply(&self.config, request(seq), Duration::from_secs(2)).await;
            if matches!(
                reply.map(|reply| reply.body),
                Some(ReplyBody::Provisional(_))
            ) {
                return;
            }
        }
        panic!("{what}: no answer in {WAIT:?}");
    }

    /// Every node up holds the same record, with at least this many results.
    async fn all_the_same(&self, results: usize, what: &str) {
        let nodes = &self.nodes;
        wait_until(WAIT, what, || {
            let views: Vec<_> = nodes
                .values()
                .map(|n| n.ctx.state.with(|s| (s.result_count(), s.digest())))
                .collect();
            views
                .iter()
                .all(|view| *view == views[0] && view.0 >= results)
        })
        .await;
    }

    /// Every node up is serving again.
    async fn all_serving(&self, what: &str) {
        let nodes = &self.nodes;
        wait_until(WAIT, what, || {
            nodes
                .values()
                .all(|n| n.ctx.serving.load(std::sync::atomic::Ordering::Relaxed))
        })
        .await;
    }

    async fn end(self) {
        for node in self.nodes.into_values() {
            node.shutdown().await;
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_killed_the_other_two_commit_and_it_returns() {
    let mut lab = Lab::start("kill-one").await;
    lab.commits("before").await;
    lab.kill(3).await;
    lab.commits("two left").await;
    lab.bring_back(3).await;
    lab.all_serving("node 3 back").await;
    lab.commits("all back").await;
    lab.all_the_same(3, "all three the same").await;
    lab.end().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_killed_the_last_answers_and_they_return_first_killed_first() {
    let mut lab = Lab::start("kill-two-a").await;
    lab.commits("before").await;
    lab.kill(3).await;
    lab.commits("two left").await;
    lab.kill(2).await;
    lab.answers_alone("one left").await;
    lab.bring_back(3).await;
    lab.commits("node 3 back with node 1").await;
    lab.bring_back(2).await;
    lab.all_serving("all back").await;
    lab.commits("all back").await;
    lab.all_the_same(4, "all three the same").await;
    lab.end().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_killed_the_last_answers_and_they_return_last_killed_first() {
    let mut lab = Lab::start("kill-two-b").await;
    lab.commits("before").await;
    lab.kill(3).await;
    lab.commits("two left").await;
    lab.kill(2).await;
    lab.answers_alone("one left").await;
    lab.bring_back(2).await;
    lab.commits("node 2 back with node 1").await;
    lab.bring_back(3).await;
    lab.all_serving("all back").await;
    lab.commits("all back").await;
    lab.all_the_same(4, "all three the same").await;
    lab.end().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_killed_at_once_and_they_return() {
    let mut lab = Lab::start("kill-two-c").await;
    lab.commits("before").await;
    lab.kill(2).await;
    lab.kill(3).await;
    lab.answers_alone("one left").await;
    lab.bring_back(2).await;
    lab.commits("node 2 back with node 1").await;
    lab.bring_back(3).await;
    lab.all_serving("all back").await;
    lab.commits("all back").await;
    lab.all_the_same(3, "all three the same").await;
    lab.end().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_kill_during_a_return_does_not_stop_them() {
    let mut lab = Lab::start("kill-return").await;
    lab.commits("before").await;
    lab.kill(3).await;
    lab.commits("two left").await;
    lab.bring_back(3).await;
    lab.kill(2).await;
    lab.commits("node 1 and node 3").await;
    lab.bring_back(2).await;
    lab.all_serving("all back").await;
    lab.commits("all back").await;
    lab.all_the_same(4, "all three the same").await;
    lab.end().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_three_killed_keep_what_they_answered_and_return_together() {
    let mut lab = Lab::start("kill-all-a").await;
    for _ in 0..5 {
        lab.commits("before").await;
    }
    lab.all_the_same(5, "all five everywhere").await;
    for id in [1, 2, 3] {
        lab.kill(id).await;
    }
    for id in [1, 2, 3] {
        lab.bring_back(id).await;
    }
    lab.commits("all back").await;
    lab.all_serving("all back").await;
    lab.all_the_same(6, "all three the same, with the old results")
        .await;
    lab.end().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_three_killed_and_brought_back_one_at_a_time() {
    let mut lab = Lab::start("kill-all-b").await;
    for _ in 0..5 {
        lab.commits("before").await;
    }
    lab.all_the_same(5, "all five everywhere").await;
    for id in [3, 2, 1] {
        lab.kill(id).await;
    }
    lab.bring_back(2).await;
    lab.answers_alone("only node 2 back").await;
    lab.bring_back(3).await;
    lab.commits("nodes 2 and 3 back").await;
    lab.bring_back(1).await;
    lab.all_serving("all back").await;
    lab.commits("all back").await;
    lab.all_the_same(7, "all three the same, with the old results")
        .await;
    lab.end().await;
}
