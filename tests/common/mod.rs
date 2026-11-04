//! Helpers for multi-node tests: real nodes in this process, on free localhost ports.
#![allow(dead_code, clippy::unwrap_used)] // each test file uses a different subset

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use rescuemesh::config::{AssignmentPolicy, ClusterConfig, Config, NodeAddr};
use rescuemesh::node::transport::{ClientPort, datagram_client};
use rescuemesh::node::{self, NodeHandle};
use rescuemesh::proto::{ClientReply, ClientRequest};
use tarpc::client::NewClient;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio::time::Instant;

pub struct TestCluster {
    pub config: Config,
    pub nodes: Vec<NodeHandle>,
}

impl TestCluster {
    pub async fn start(n: u64) -> Self {
        Self::start_with(n, |_| {}).await
    }

    /// Start `n` nodes; `tweak` can adjust the config before the nodes see it.
    pub async fn start_with(n: u64, tweak: impl FnOnce(&mut Config)) -> Self {
        Self::start_each(n, tweak, |_, _| {}).await
    }

    /// As [`start_with`](Self::start_with), and `each` can change what one node reads, the way a
    /// file copied wrongly to one machine would. `TestCluster::config` stays the shared one.
    pub async fn start_each(
        n: u64,
        tweak: impl FnOnce(&mut Config),
        each: impl Fn(u64, &mut Config),
    ) -> Self {
        let mut listeners = Vec::new();
        let mut addrs = Vec::new();
        for id in 1..=n {
            let peer = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = ClientPort::bind("127.0.0.1:0").await.unwrap();
            addrs.push(NodeAddr {
                id,
                peer_addr: peer.local_addr().unwrap().to_string(),
                client_addr: client.local_addr().unwrap().to_string(),
            });
            listeners.push((id, peer, client));
        }
        let mut config = Config {
            cluster: ClusterConfig { nodes: addrs },
            timers: Default::default(),
            consensus: Default::default(),
            policy: Default::default(),
            workers: Default::default(),
            campus: Default::default(),
            metrics: Default::default(),
            faults: Default::default(),
            storage: Default::default(),
        };
        // Most tests are written against the simple baseline; the bidding tests set their own.
        config.policy.assignment = AssignmentPolicy::LeaderDispatch;
        tweak(&mut config);
        config.validate().unwrap();

        let mut nodes = Vec::new();
        for (id, peer, client) in listeners {
            let mut own = config.clone();
            each(id, &mut own);
            nodes.push(node::start_with(own, id, peer, client).await.unwrap());
        }
        Self { config, nodes }
    }

    pub fn node(&self, id: u64) -> &NodeHandle {
        &self.nodes[(id - 1) as usize]
    }

    pub async fn shutdown(self) {
        for n in self.nodes {
            n.shutdown().await;
        }
    }
}

/// Poll `cond` every 10 ms until it holds; panic with `what` after `timeout`.
pub async fn wait_until(timeout: Duration, what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !cond() {
        if Instant::now() > deadline {
            panic!("timed out after {timeout:?} waiting for {what}");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Runs `jobs` jobs on the node's worker pool that last until the returned senders are dropped.
pub async fn hold_jobs(node: &NodeHandle, jobs: usize) -> Vec<std::sync::mpsc::Sender<()>> {
    let before = node.ctx.workers.load();
    let mut senders = Vec::new();
    for _ in 0..jobs {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let pool = node.ctx.workers.clone();
        tokio::spawn(async move {
            let _ = pool
                .run(move || {
                    let _ = rx.recv();
                })
                .await;
        });
        senders.push(tx);
    }
    wait_until(
        Duration::from_secs(5),
        "the held jobs to reach the pool",
        || {
            let load = node.ctx.workers.load();
            load.queued + load.running == before.queued + before.running + jobs
        },
    )
    .await;
    senders
}

/// Send `request` to every node's client port and return the first reply, if any arrives
/// within `timeout`. A stand-in for the real client middleware, with no retry: one datagram per node,
/// so a test that needs a copy to survive a full buffer should go through `MeshClient`.
pub async fn first_reply(
    config: &Config,
    request: ClientRequest,
    timeout: Duration,
) -> Option<ClientReply> {
    let mut stubs = Vec::new();
    // Dropped on return, which ends each stub's work and so every call.
    let mut dispatches = JoinSet::new();
    for n in &config.cluster.nodes {
        let NewClient { client, dispatch } =
            datagram_client(&n.client_addr, Arc::default(), None).unwrap();
        dispatches.spawn(dispatch);
        stubs.push(client);
    }
    let deadline = std::time::Instant::now() + timeout;
    let mut calls: FuturesUnordered<_> = stubs
        .iter()
        .map(|stub| {
            let mut ctx = tarpc::context::current();
            ctx.deadline = deadline;
            stub.submit(ctx, request.clone())
        })
        .collect();
    let first = async {
        while let Some(result) = calls.next().await {
            if let Ok(reply) = result {
                return Some(reply);
            }
        }
        None
    };
    tokio::time::timeout(timeout, first).await.ok().flatten()
}
