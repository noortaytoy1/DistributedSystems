//! Helpers for multi-node tests: real nodes in this process, on free localhost ports.
#![allow(dead_code, clippy::unwrap_used)] // each test file uses a different subset

use std::time::Duration;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use rescuemesh::config::{ClusterConfig, Config, NodeAddr};
use rescuemesh::node::transport::framed;
use rescuemesh::node::{self, NodeHandle};
use rescuemesh::proto::{self, ClientFrame, ClientReply, ClientRequest, ServerFrame};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
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
        let mut listeners = Vec::new();
        let mut addrs = Vec::new();
        for id in 1..=n {
            let peer = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = TcpListener::bind("127.0.0.1:0").await.unwrap();
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
            policy: Default::default(),
            workers: Default::default(),
            campus: Default::default(),
            metrics: Default::default(),
        };
        tweak(&mut config);
        config.validate().unwrap();

        let mut nodes = Vec::new();
        for (id, peer, client) in listeners {
            nodes.push(
                node::start_with(config.clone(), id, peer, client)
                    .await
                    .unwrap(),
            );
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

/// Send `request` to every node's client port and return the first reply, if any arrives
/// within `timeout`. A stand-in for the real client middleware.
pub async fn first_reply(
    config: &Config,
    request: ClientRequest,
    timeout: Duration,
) -> Option<ClientReply> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let bytes = Bytes::from(proto::encode(&ClientFrame::Request(request)).unwrap());
    let mut tasks = Vec::new();
    for n in &config.cluster.nodes {
        let (addr, tx, bytes) = (n.client_addr.clone(), tx.clone(), bytes.clone());
        tasks.push(tokio::spawn(async move {
            let Ok(stream) = TcpStream::connect(&addr).await else {
                return;
            };
            let mut conn = framed(stream);
            if conn.send(bytes).await.is_err() {
                return;
            }
            while let Some(Ok(frame)) = conn.next().await {
                if let Ok(ServerFrame::Reply(reply)) = proto::decode(&frame) {
                    let _ = tx.send(reply);
                    return;
                }
            }
        }));
    }
    drop(tx);
    let reply = tokio::time::timeout(timeout, rx.recv())
        .await
        .ok()
        .flatten();
    for t in tasks {
        t.abort();
    }
    reply
}
