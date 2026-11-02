//! Client middleware: multicast each request to every node, retry with the same request ID,
//! keep the first reply. The client never names a node.
//!
//! The application makes one `submit` call; this layer copies the request to every replica. One
//! [`MeshClient`] holds one connection per node, shared by all simulated users through `Arc`
//! (D53). Replies are matched to waiting users by request ID.
//!
//! - **Retries (D52).** Up to `client_max_retries` resends with the same ID and `attempt + 1`.
//!   The wait starts at `client_retry_ms` and doubles up to `client_retry_max_ms`, ±20% jitter.
//! - **Dead nodes (D112).** Each connection pings with `Control(Status)`. Control frames pass the
//!   fault gate, so a silenced node keeps its connection; a machine that lost power stops
//!   answering, and the connection is replaced.
//! - **Provisional answers (D123).** A lone node's answer counts as a reply, because no commit can
//!   come. `Submitted` keeps it apart from a committed result.
//! - **Backpressure (D112).** Each node has a bounded send queue. A full queue drops the frame for
//!   that node only, so a dead node never blocks a user.
//!
//! Owner: F (client and scale).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use rand::Rng;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::config::Config;
use crate::metrics::{Event, EventLog, now_us};
use crate::node::transport::framed;
use crate::proto::{
    self, ClientFrame, ClientId, ClientReply, ClientRequest, CommittedResult, ControlReply,
    ControlRequest, Incident, NodeId, Plan, ReplyBody, RequestId, ServerFrame,
};

/// Frames waiting for one node. Frames beyond this are dropped; the retry covers them.
const SEND_QUEUE: usize = 1024;
/// How often a connection pings its node.
const PING_INTERVAL: Duration = Duration::from_secs(1);
/// A connection that receives nothing for this long is replaced. Also caps one socket write.
const PING_TIMEOUT: Duration = Duration::from_secs(3);
/// Cap on one connect attempt, and the pause after a failed one.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const CONNECT_PAUSE: Duration = Duration::from_millis(500);
/// Each wait varies by up to this share, in percent, so retries do not arrive together.
const JITTER_PERCENT: u64 = 20;

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
}

/// A request that got a reply.
#[derive(Debug, Clone)]
pub struct Submitted {
    /// The node that answered first.
    pub from: NodeId,
    /// The node that planned the request.
    pub executor: NodeId,
    pub plan: Plan,
    /// The committed result; `None` for a provisional answer, which has none yet.
    pub committed: Option<CommittedResult>,
    /// True if a lone node answered without a commit (D123).
    pub provisional: bool,
    /// From first send to first reply, on the client clock.
    pub latency: Duration,
    pub attempts: u32,
}

type Waiters = Arc<Mutex<HashMap<RequestId, oneshot::Sender<ClientReply>>>>;

/// One client process's connection to the cloud. Share it between simulated users with `Arc`.
pub struct MeshClient {
    id: ClientId,
    retry: Duration,
    retry_max: Duration,
    max_retries: u32,
    log: EventLog,
    next_seq: AtomicU64,
    waiters: Waiters,
    links: Vec<mpsc::Sender<Bytes>>,
    tasks: Vec<JoinHandle<()>>,
}

impl MeshClient {
    /// Start one connection task per node in the config. Must run inside a tokio runtime.
    pub fn new(id: ClientId, config: &Config, log: EventLog) -> Self {
        let waiters = Waiters::default();
        let mut links = Vec::new();
        let mut tasks = Vec::new();
        for node in &config.cluster.nodes {
            let (tx, rx) = mpsc::channel(SEND_QUEUE);
            links.push(tx);
            tasks.push(tokio::spawn(run_link(
                node.id,
                node.client_addr.clone(),
                rx,
                waiters.clone(),
                log.clone(),
            )));
        }
        Self {
            id,
            retry: config.timers.client_retry(),
            retry_max: config.timers.client_retry_max(),
            max_retries: config.timers.client_max_retries,
            log,
            // Starts at the current time, so a rerun with the same client ID can never reuse a
            // request ID the cluster has already answered (D113).
            next_seq: AtomicU64::new(now_us()),
            waiters,
            links,
            tasks,
        }
    }

    /// Submit an incident and wait for the first reply. Latency counts from now.
    pub async fn submit(&self, incident: Incident) -> Result<Submitted, ClientError> {
        self.submit_from(incident, Instant::now()).await
    }

    /// As [`submit`](Self::submit), with latency counted from `started`. An open-loop generator
    /// passes the intended send time, so queueing in the generator is not hidden (D54).
    pub async fn submit_from(
        &self,
        incident: Incident,
        started: Instant,
    ) -> Result<Submitted, ClientError> {
        let id = RequestId {
            client: self.id,
            seq: self.next_seq.fetch_add(1, Ordering::Relaxed),
        };
        let (reply_tx, mut reply_rx) = oneshot::channel();
        lock(&self.waiters).insert(id, reply_tx);
        // Removes the waiter however this call ends, including if the caller drops it.
        let _waiting = Waiting {
            waiters: &self.waiters,
            id,
        };

        let attempts = self.max_retries + 1;
        for attempt in 0..attempts {
            self.send_to_all(id, attempt, &incident)?;
            let wait = jittered(self.wait_before_retry(attempt));
            if let Ok(reply) = tokio::time::timeout(wait, &mut reply_rx).await {
                let reply = reply.map_err(|_| ClientError::Closed)?;
                return Ok(self.finished(reply, started, attempt + 1));
            }
        }
        self.log.log(Event::ClientGaveUp {
            request: id,
            attempts,
        });
        Err(ClientError::GaveUp { attempts })
    }

    fn send_to_all(
        &self,
        id: RequestId,
        attempt: u32,
        incident: &Incident,
    ) -> Result<(), ClientError> {
        let frame = ClientFrame::Request(ClientRequest {
            id,
            attempt,
            incident: incident.clone(),
        });
        let bytes = Bytes::from(proto::encode(&frame)?);
        self.log.log(Event::ClientSent {
            request: id,
            attempt,
        });
        for link in &self.links {
            if link.try_send(bytes.clone()).is_err() {
                tracing::debug!(request = %id, "send queue full: frame dropped for one node");
            }
        }
        Ok(())
    }

    /// Doubles from `client_retry_ms` up to `client_retry_max_ms`.
    fn wait_before_retry(&self, attempt: u32) -> Duration {
        self.retry
            .saturating_mul(2u32.saturating_pow(attempt))
            .min(self.retry_max)
    }

    fn finished(&self, reply: ClientReply, started: Instant, attempts: u32) -> Submitted {
        let (executor, plan, committed) = match reply.body {
            ReplyBody::Done(result) => (result.executor, result.plan.clone(), Some(result)),
            // A lone node plans for itself.
            ReplyBody::Provisional(plan) => (reply.from, plan, None),
            ReplyBody::Rejected(_) => unreachable!("run_link does not hand over Rejected replies"),
        };
        let provisional = committed.is_none();
        let latency = started.elapsed();
        self.log.log(Event::ClientDone {
            request: reply.id,
            from: reply.from,
            executor,
            latency_us: latency.as_micros() as u64,
            attempts,
            provisional,
        });
        Submitted {
            from: reply.from,
            executor,
            plan,
            committed,
            provisional,
            latency,
            attempts,
        }
    }
}

impl Drop for MeshClient {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

struct Waiting<'a> {
    waiters: &'a Waiters,
    id: RequestId,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        lock(self.waiters).remove(&self.id);
    }
}

fn jittered(wait: Duration) -> Duration {
    let percent = rand::thread_rng().gen_range(100 - JITTER_PERCENT..=100 + JITTER_PERCENT);
    wait.saturating_mul(percent as u32) / 100
}

/// Keep one connection to a node open for as long as the client lives. Frames wait in the queue
/// while the node is unreachable; resending them later is harmless, because requests are
/// idempotent.
async fn run_link(
    node: NodeId,
    addr: String,
    mut frames: mpsc::Receiver<Bytes>,
    waiters: Waiters,
    log: EventLog,
) {
    loop {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&addr)).await;
        let Ok(Ok(stream)) = stream else {
            tokio::time::sleep(CONNECT_PAUSE).await;
            continue;
        };
        log.log(Event::ClientLinkUp { node });
        let reason = serve_link(stream, &mut frames, &waiters).await;
        // The client is going away; this is no failure of the node.
        if frames.is_closed() {
            return;
        }
        tracing::debug!(node, "client connection lost: {reason}");
        log.log(Event::ClientLinkDown {
            node,
            reason: reason.to_string(),
        });
        tokio::time::sleep(CONNECT_PAUSE).await;
    }
}

/// Runs one connection until it fails, and says why.
async fn serve_link(
    stream: TcpStream,
    frames: &mut mpsc::Receiver<Bytes>,
    waiters: &Waiters,
) -> &'static str {
    let Ok(ping) = proto::encode(&ClientFrame::Control(ControlRequest::Status)) else {
        return "cannot encode ping";
    };
    let ping = Bytes::from(ping);
    let mut conn = framed(stream);
    let mut ticks = tokio::time::interval(PING_INTERVAL);
    let mut heard = Instant::now();
    loop {
        let outgoing = tokio::select! {
            incoming = conn.next() => {
                let Some(Ok(frame)) = incoming else {
                    return "closed by the node";
                };
                heard = Instant::now();
                match proto::decode::<ServerFrame>(&frame) {
                    Ok(ServerFrame::Reply(reply)) => hand_over(waiters, reply),
                    Ok(ServerFrame::Control(_)) => {}
                    Err(_) => return "bad frame",
                }
                continue;
            }
            _ = ticks.tick() => {
                if heard.elapsed() >= PING_TIMEOUT {
                    return "no answer to pings";
                }
                ping.clone()
            }
            frame = frames.recv() => match frame {
                Some(frame) => frame,
                None => return "client dropped",
            },
        };
        // A dead peer's socket eventually stops taking data; never wait on it for long.
        match tokio::time::timeout(PING_TIMEOUT, conn.send(outgoing)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return "send failed",
            Err(_) => return "send stalled",
        }
    }
}

/// The first `Done` or `Provisional` for an ID wins: it takes the waiter, so later ones find
/// nothing. `Rejected` is not an answer (D36): the client keeps waiting for another node or its
/// retry.
fn hand_over(waiters: &Waiters, reply: ClientReply) {
    if matches!(reply.body, ReplyBody::Rejected(_)) {
        return;
    }
    if let Some(waiter) = lock(waiters).remove(&reply.id) {
        let _ = waiter.send(reply);
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

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_stays_within_twenty_percent() {
        let wait = Duration::from_millis(1000);
        for _ in 0..1000 {
            let j = jittered(wait);
            assert!(j >= Duration::from_millis(800) && j <= Duration::from_millis(1200));
        }
    }
}
