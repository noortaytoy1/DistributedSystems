//! Client middleware: multicast each request to every node, retry with the same request ID,
//! keep the first reply. The client never names a node.
//!
//! Owner: F (client and scale). Template: `MeshClient::submit` is not implemented; `control`
//! (used by `ctl`) works.

use std::time::Duration;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;

use crate::config::Config;
use crate::metrics::EventLog;
use crate::node::transport::framed;
use crate::proto::{
    self, ClientFrame, ClientId, ClientReply, ControlReply, ControlRequest, Incident, ServerFrame,
};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("no reply after {attempts} attempts")]
    GaveUp { attempts: u32 },
    #[error("cannot connect to {addr}: {source}")]
    Connect {
        addr: String,
        source: std::io::Error,
    },
    #[error("timed out")]
    Timeout,
    #[error("connection closed")]
    Closed,
    #[error("encoding: {0}")]
    Wire(#[from] proto::WireError),
    #[error("not implemented yet")]
    NotImplemented,
}

/// A request that got a reply.
#[derive(Debug, Clone)]
pub struct Submitted {
    pub reply: ClientReply,
    /// From first send to first reply, on the client clock.
    pub latency: Duration,
    pub attempts: u32,
}

/// One client process's connection to the cloud. Share it between simulated users with `Arc`.
pub struct MeshClient {
    id: ClientId,
    node_addrs: Vec<String>,
    retry: Duration,
    max_retries: u32,
    log: EventLog,
}

impl MeshClient {
    pub fn new(id: ClientId, config: &Config, log: EventLog) -> Self {
        Self {
            id,
            node_addrs: config
                .cluster
                .nodes
                .iter()
                .map(|n| n.client_addr.clone())
                .collect(),
            retry: config.timers.client_retry(),
            max_retries: config.timers.client_max_retries,
            log,
        }
    }

    /// Submit an incident and wait for the first reply.
    pub async fn submit(&self, _incident: Incident) -> Result<Submitted, ClientError> {
        // TODO(F): new `RequestId { client: self.id, seq }`; send `ClientFrame::Request` to every
        // address in `node_addrs`; wait `retry` for the first `ServerFrame::Reply` with that ID;
        // resend with the same ID up to `max_retries` times; log `ClientSent`, `ClientDone` and
        // `ClientGaveUp` to `log`. Keep one connection per node, shared by all users.
        let _ = (
            self.id,
            &self.node_addrs,
            self.retry,
            self.max_retries,
            &self.log,
        );
        Err(ClientError::NotImplemented)
    }
}

/// Send one control command to one node and wait for its answer (used by `ctl`).
pub async fn control(
    addr: &str,
    request: ControlRequest,
    timeout: Duration,
) -> Result<ControlReply, ClientError> {
    let exchange = async {
        let stream = TcpStream::connect(addr)
            .await
            .map_err(|source| ClientError::Connect {
                addr: addr.to_string(),
                source,
            })?;
        let mut framed = framed(stream);
        let bytes = proto::encode(&ClientFrame::Control(request))?;
        framed
            .send(Bytes::from(bytes))
            .await
            .map_err(|_| ClientError::Closed)?;
        while let Some(Ok(frame)) = framed.next().await {
            if let ServerFrame::Control(reply) = proto::decode(&frame)? {
                return Ok(reply);
            }
        }
        Err(ClientError::Closed)
    };
    tokio::time::timeout(timeout, exchange)
        .await
        .map_err(|_| ClientError::Timeout)?
}
