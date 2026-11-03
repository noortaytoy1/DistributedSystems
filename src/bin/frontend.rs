//! The web page on a client PC: report an incident and read its plan, start real user programs,
//! and watch the nodes. One frontend per client PC, driving only that PC (D197).
//!
//! ```text
//! browser --HTTP on this PC--> frontend --MeshClient--> node 1, 2, 3
//!                                 \--> up to 10 real loadgen programs per click
//! ```
//!
//! ```text
//! target/release/frontend --config config/lab.toml --client-id 11 --listen 127.0.0.1:9001
//! ```
//!
//! - **It is a client.** A report goes through [`MeshClient::submit`] only, so the page cannot name
//!   a node. The status table uses `client::control`, as `ctl` does, and is an operator's view.
//! - **Sockets (D200).** This file is a named exception to the rule that only `node/` and
//!   `raft/network.rs` open sockets: it only listens for the browser on its own PC and carries no
//!   cloud traffic.
//! - **HTTP (D198).** About 250 hand-written lines on tokio, with no new crate. One request per
//!   connection and no request bodies: values travel in the query string, and a parameter this
//!   program does not know gets 400, so `node=2` can never mean anything.
//! - **Load box (D202).** Each click starts up to 10 real `loadgen` programs sharing the users:
//!   1000 users are 10 programs of 100. The programs are children that die with this one.
//! - **The page (`frontend.html`).** One embedded file with plain JavaScript that writes through
//!   `textContent` only and has no outside links, because the lab network may have no internet.
//!
//! There is no panel for following a zone: that is publish-subscribe, which is Phase 2.
//!
//! Owner: F (client and scale).

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use futures::future::join_all;
use rescuemesh::campus;
use rescuemesh::client::{ClientError, MeshClient, Submitted, control};
use rescuemesh::config::{CampusConfig, Config};
use rescuemesh::metrics::{EventLog, now_ms};
use rescuemesh::proto::{
    Capability, ClientId, ControlReply, ControlRequest, Incident, IncidentStatus, NodeStatus,
    Responder, ResponderId, UserId, ZoneId,
};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio::time::timeout;

// ---------------------------------------------------------------------------------------------
// Limits. Constants, as in D112: no experiment varies them, so `Config` stays unchanged.
// ---------------------------------------------------------------------------------------------

/// Largest request head. The API has no bodies, so a browser's head fits easily.
const MAX_HEAD_BYTES: usize = 8 * 1024;
/// A browser that opened a connection and says nothing must not hold a slot for long.
const HEAD_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// One page uses a handful; a report can hold one for its whole 36 s retry budget.
const MAX_CONNECTIONS: usize = 64;
/// One dead node costs one poll this much, and no more: nodes are asked at the same time.
const STATUS_TIMEOUT: Duration = Duration::from_millis(800);

const SEVERITY_LABELS: [(u8, &str); 5] = [
    (1, "minor"),
    (2, "low"),
    (3, "serious"),
    (4, "severe"),
    (5, "life-threatening"),
];
const DEFAULT_WHO: u32 = 1;
/// Noor's item 5: longer than the client's 30 s of retries.
const DEFAULT_VALID_FOR_S: u64 = 60;
/// 0 would be expired before it left, which a person typing a report never means (D143).
const MIN_VALID_FOR_S: u64 = 1;
const MAX_VALID_FOR_S: u64 = 3600;

const DEFAULT_USERS: u32 = 30;
const DEFAULT_THINK_MS: u64 = 1000;
const DEFAULT_DURATION_S: u64 = 600;
/// Programs one click starts (Noor's item 3): 1000 users are 10 programs of 100.
const PROGRAMS_PER_CLICK: u32 = 10;
/// Above these one client PC chokes first, and overload can fake degraded mode (runbook).
const MAX_RUNNING_USERS: u32 = 1000;
const MAX_RUNNING_PROGRAMS: usize = 20;
const MAX_OFFERED_REPORTS_PER_S: u64 = 1500;
const MIN_THINK_MS: u64 = 200;
const MAX_DURATION_S: u64 = 900;
/// A program's ID is `client_id * this + n`, unique across PCs. Six digits for the IDs 11, 12 and 13
/// the docs use, because five-digit IDs belong to `scripts/start-users.sh`: at 1000, 11001 would be
/// its machine 1, batch 1, program 1, and two programs would share one ID and one log.
const PROGRAM_ID_BASE: u64 = 10_000;
const MAX_PROGRAM_NUMBER: u64 = PROGRAM_ID_BASE - 1;
/// Rows kept for the page; the oldest ended ones go first.
const MAX_PROGRAM_ROWS: usize = 60;
const MAX_SUMMARY_BYTES: usize = 4096;
/// After TERM. loadgen prints its summary in this time (D183); anything still running is killed.
const STOP_GRACE: Duration = Duration::from_secs(2);
const TERM_TIMEOUT: Duration = Duration::from_secs(1);

const PAGE: &str = include_str!("frontend.html");

/// Every path this program answers, and its one method.
const ROUTES: &[(&str, &str)] = &[
    ("/", "GET"),
    ("/api/info", "GET"),
    ("/api/report", "POST"),
    ("/api/status", "GET"),
    ("/api/load", "GET"),
    ("/api/load/start", "POST"),
    ("/api/load/stop", "POST"),
];

// ---------------------------------------------------------------------------------------------
// Program
// ---------------------------------------------------------------------------------------------

#[derive(Parser)]
#[command(about = "Web page for one client PC: report incidents, run user programs, watch nodes")]
struct Args {
    /// Address for the browser on this PC, e.g. 127.0.0.1:9001. The cluster never connects here,
    /// so it is not in the shared config (D199).
    #[arg(long)]
    listen: String,
    /// Required: a forgotten flag on a lab PC must not load loopback addresses and show three
    /// unreachable nodes.
    #[arg(long)]
    config: PathBuf,
    /// Unique across machines (D113). Programs started here use this times 10000, plus a number.
    #[arg(long)]
    client_id: ClientId,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rescuemesh::init_tracing();
    let args = Args::parse();
    let config =
        Config::load(&args.config).with_context(|| format!("loading {}", args.config.display()))?;
    anyhow::ensure!(
        args.client_id
            .checked_mul(PROGRAM_ID_BASE)
            .and_then(|id| id.checked_add(MAX_PROGRAM_NUMBER))
            .is_some(),
        "--client-id is too large: programs use it times 10000"
    );
    let log = EventLog::open(
        config.metrics.dir.as_deref(),
        &format!("frontend-{}", args.client_id),
    )
    .context("opening the event log")?;
    let exe = std::env::current_exe().context("finding this program's folder")?;
    let loadgen = exe.with_file_name(format!("loadgen{}", std::env::consts::EXE_SUFFIX));
    let listener = TcpListener::bind(&args.listen)
        .await
        .with_context(|| format!("listening on {}", args.listen))?;
    let addr = listener.local_addr().context("reading the bound address")?;

    let app = Arc::new(App::new(
        config,
        args.config,
        args.client_id,
        log.clone(),
        loadgen,
    ));
    // The first line on stdout. Tests read the port from it when --listen asks for port 0.
    println!("listening on {addr}");
    tracing::info!(%addr, "frontend started");

    serve(listener, app.clone(), rescuemesh::shutdown_signal()).await;

    tracing::info!("frontend stopping");
    app.shutdown().await;
    drop(app);
    tokio::task::spawn_blocking(move || log.close())
        .await
        .context("closing the event log")?;
    Ok(())
}

struct App {
    config: Config,
    config_path: PathBuf,
    client_id: ClientId,
    client: MeshClient,
    responders: BTreeMap<ResponderId, Responder>,
    loadgen: PathBuf,
    load: Arc<Mutex<LoadBox>>,
}

impl App {
    /// Must run inside a tokio runtime, because `MeshClient::new` starts its link tasks.
    fn new(
        config: Config,
        config_path: PathBuf,
        client_id: ClientId,
        log: EventLog,
        loadgen: PathBuf,
    ) -> Self {
        let client = MeshClient::new(client_id, &config, log);
        let responders = campus::responders(&config.campus)
            .into_iter()
            .map(|r| (r.id, r))
            .collect();
        Self {
            config,
            config_path,
            client_id,
            client,
            responders,
            loadgen,
            load: Arc::default(),
        }
    }

    /// End every program. Runs after the last connection is gone.
    async fn shutdown(&self) {
        let mut tasks = {
            let mut load = lock(&self.load);
            load.stop_all();
            std::mem::take(&mut load.tasks)
        };
        let ended = timeout(STOP_GRACE + TERM_TIMEOUT, async {
            while tasks.join_next().await.is_some() {}
        })
        .await;
        if ended.is_err() {
            tracing::warn!("a program did not end in time: killing it");
        }
        // Dropping the set aborts what is left, and that drops the child, which kills it.
        drop(tasks);
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------------------------------------
// HTTP. One request per connection, no bodies, one kind of client.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct Request {
    method: String,
    path: String,
    query: Vec<(String, String)>,
}

#[derive(Debug, PartialEq, Eq)]
enum HeadError {
    /// The peer went away before finishing; nobody is left to answer.
    Closed,
    TooLarge,
    Malformed(&'static str),
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    allow: Option<&'static str>,
}

impl Response {
    fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: value.to_string().into_bytes(),
            allow: None,
        }
    }

    fn error(status: u16, message: impl Into<String>) -> Self {
        Self::json(status, &json!({ "ok": false, "error": message.into() }))
    }

    fn page() -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: PAGE.as_bytes().to_vec(),
            allow: None,
        }
    }

    fn wrong_method(allowed: &'static str) -> Self {
        Self {
            allow: Some(allowed),
            ..Self::error(405, format!("this path only takes {allowed}"))
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let reason = match self.status {
            200 => "OK",
            400 => "Bad Request",
            404 => "Not Found",
            405 => "Method Not Allowed",
            408 => "Request Timeout",
            431 => "Request Header Fields Too Large",
            500 => "Internal Server Error",
            501 => "Not Implemented",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            504 => "Gateway Timeout",
            _ => "Unknown",
        };
        let mut head = format!(
            "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
             Connection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n",
            self.status,
            self.content_type,
            self.body.len()
        );
        if let Some(allowed) = self.allow {
            head.push_str(&format!("Allow: {allowed}\r\n"));
        }
        if self.content_type.starts_with("text/html") {
            // The browser itself holds the page to "no outside links".
            head.push_str(
                "Content-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; \
                 style-src 'unsafe-inline'; connect-src 'self'; base-uri 'none'; \
                 form-action 'none'\r\n",
            );
        }
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(&self.body);
        bytes
    }
}

/// Read up to the blank line, over as many reads as it takes. The caller caps the time.
async fn read_head(stream: &mut (impl AsyncRead + Unpin)) -> Result<Vec<u8>, HeadError> {
    const END: &[u8] = b"\r\n\r\n";
    let mut head = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(at) = head.windows(END.len()).position(|w| w == END) {
            if at + END.len() > MAX_HEAD_BYTES {
                return Err(HeadError::TooLarge);
            }
            head.truncate(at);
            return Ok(head);
        }
        if head.len() >= MAX_HEAD_BYTES {
            return Err(HeadError::TooLarge);
        }
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return Err(HeadError::Closed),
            Ok(n) => head.extend_from_slice(&chunk[..n]),
        }
    }
}

fn parse_head(head: &[u8]) -> Result<Request, HeadError> {
    let text = std::str::from_utf8(head).map_err(|_| HeadError::Malformed("not text"))?;
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(HeadError::Malformed("bad request line"));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(HeadError::Malformed("only HTTP/1 is spoken here"));
    }
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(HeadError::Malformed("bad header line"));
        };
        // A body would go unread, and closing on unread bytes can destroy the answer on its way.
        let has_body = (name.eq_ignore_ascii_case("content-length") && value.trim() != "0")
            || name.eq_ignore_ascii_case("transfer-encoding");
        if has_body {
            return Err(HeadError::Malformed(
                "this API takes no body: values go in the query string",
            ));
        }
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let query = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (name.to_string(), value.to_string())
        })
        .collect();
    Ok(Request {
        method: method.to_string(),
        path: path.to_string(),
        query,
    })
}

/// Reads the query string. Every value is digits or an enum name, so nothing needs decoding: an
/// encoded value simply fails its check.
struct Query<'a>(&'a [(String, String)]);

impl Query<'_> {
    /// Refuse anything else, and any repeat, so a parameter can never be silently ignored.
    fn only(&self, allowed: &[&str]) -> Result<(), Response> {
        let mut seen = BTreeSet::new();
        for (name, _) in self.0 {
            if !allowed.contains(&name.as_str()) {
                return Err(Response::error(400, format!("unknown parameter {name}")));
            }
            if !seen.insert(name.as_str()) {
                return Err(Response::error(400, format!("{name} is given twice")));
            }
        }
        Ok(())
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, value)| value.as_str())
    }

    fn number<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>, Response> {
        self.get(name)
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| Response::error(400, format!("{name} must be a whole number")))
            })
            .transpose()
    }

    fn required_number<T: std::str::FromStr>(&self, name: &str) -> Result<T, Response> {
        self.number(name)?
            .ok_or_else(|| Response::error(400, format!("{name} is missing")))
    }
}

/// Accept until `shutdown` resolves, then end every connection.
async fn serve(listener: TcpListener, app: Arc<App>, shutdown: impl Future<Output = ()>) {
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    while connections.try_join_next().is_some() {}
                    if connections.len() >= MAX_CONNECTIONS {
                        // Never wait here: a full socket must not stop the accept loop.
                        let busy = Response::error(503, "too many open connections").bytes();
                        let _ = stream.try_write(&busy);
                    } else {
                        connections.spawn(connection(stream, app.clone()));
                    }
                }
                Err(e) => {
                    tracing::warn!("accept failed: {e}");
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}

async fn connection(mut stream: TcpStream, app: Arc<App>) {
    let response = match timeout(HEAD_TIMEOUT, read_head(&mut stream)).await {
        Err(_) => Response::error(408, "no complete request in time"),
        Ok(Err(HeadError::Closed)) => return,
        Ok(Err(HeadError::TooLarge)) => Response::error(431, "request head too large"),
        Ok(Err(HeadError::Malformed(why))) => Response::error(400, why),
        Ok(Ok(head)) => match parse_head(&head) {
            Ok(request) => route(&app, &request).await,
            Err(HeadError::Malformed(why)) => Response::error(400, why),
            Err(_) => Response::error(400, "bad request"),
        },
    };
    let write = async {
        stream.write_all(&response.bytes()).await?;
        stream.shutdown().await
    };
    match timeout(WRITE_TIMEOUT, write).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::debug!("browser went away: {e}"),
        Err(_) => tracing::debug!("browser stopped reading"),
    }
}

async fn route(app: &App, request: &Request) -> Response {
    let Some((path, method)) = ROUTES.iter().find(|(path, _)| *path == request.path) else {
        return Response::error(404, format!("no such path: {}", request.path));
    };
    if *method != request.method {
        return Response::wrong_method(method);
    }
    let query = Query(&request.query);
    let answered = match *path {
        "/" => query.only(&[]).map(|()| Response::page()),
        "/api/info" => query.only(&[]).map(|()| info(app)),
        "/api/report" => report(app, &query).await,
        "/api/status" => status(app, &query).await,
        "/api/load" => query
            .only(&[])
            .map(|()| Response::json(200, &lock(&app.load).to_json())),
        "/api/load/start" => load_start(app, &query),
        "/api/load/stop" => query.only(&[]).map(|()| load_stop(app)),
        _ => Ok(Response::error(404, "no such path")),
    };
    answered.unwrap_or_else(|refused| refused)
}

/// What the page needs to draw itself, so the form can only offer what the cloud will accept.
fn info(app: &App) -> Response {
    let patterns: Vec<Value> = Pattern::ALL
        .into_iter()
        .map(|p| {
            json!({
                "name": p.name(),
                "label": p.label(),
            })
        })
        .collect();
    let severities: Vec<Value> = SEVERITY_LABELS
        .iter()
        .map(|(value, label)| json!({ "value": value, "label": label }))
        .collect();
    let capabilities: Vec<String> = Capability::ALL.iter().map(capability_name).collect();
    Response::json(
        200,
        &json!({
            "ok": true,
            "client_id": app.client_id,
            "zones": app.config.campus.zones,
            "severities": severities,
            "capabilities": capabilities,
            "patterns": patterns,
            "defaults": {
                "who": DEFAULT_WHO,
                "valid_for_s": DEFAULT_VALID_FOR_S,
                "users": DEFAULT_USERS,
                "think_ms": DEFAULT_THINK_MS,
                "duration_s": DEFAULT_DURATION_S,
            },
            "limits": {
                "valid_for_s_min": MIN_VALID_FOR_S,
                "valid_for_s_max": MAX_VALID_FOR_S,
                "running_users": MAX_RUNNING_USERS,
                "running_programs": MAX_RUNNING_PROGRAMS,
                "offered_reports_per_s": MAX_OFFERED_REPORTS_PER_S,
                "think_ms_min": MIN_THINK_MS,
                "duration_s_max": MAX_DURATION_S,
                "programs_per_click": PROGRAMS_PER_CLICK,
            },
        }),
    )
}

// ---------------------------------------------------------------------------------------------
// Report form and answer card
// ---------------------------------------------------------------------------------------------

/// What the person typed, after its checks. Nodes check none of this, so it happens here (D201).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReportForm {
    zone: ZoneId,
    severity: u8,
    needs: Capability,
    who: u32,
    valid_for_s: u64,
}

fn capability_name(capability: &Capability) -> String {
    format!("{capability:?}")
}

fn parse_report(query: &Query<'_>, campus: &CampusConfig) -> Result<ReportForm, Response> {
    query.only(&["zone", "severity", "needs", "who", "valid_for_s"])?;
    let zone: ZoneId = query.required_number("zone")?;
    if zone >= campus.zones {
        return Err(Response::error(
            400,
            format!("zone must be below {}", campus.zones),
        ));
    }
    let severity: u8 = query.required_number("severity")?;
    if !SEVERITY_LABELS.iter().any(|(value, _)| *value == severity) {
        return Err(Response::error(400, "severity must be 1 to 5"));
    }
    let needs = query
        .get("needs")
        .ok_or_else(|| Response::error(400, "needs is missing"))?;
    let needs = Capability::ALL
        .into_iter()
        .find(|c| capability_name(c) == needs)
        .ok_or_else(|| Response::error(400, format!("unknown capability {needs}")))?;
    let who = query.number("who")?.unwrap_or(DEFAULT_WHO);
    let valid_for_s = query.number("valid_for_s")?.unwrap_or(DEFAULT_VALID_FOR_S);
    if !(MIN_VALID_FOR_S..=MAX_VALID_FOR_S).contains(&valid_for_s) {
        return Err(Response::error(
            400,
            format!("valid-for must be {MIN_VALID_FOR_S} to {MAX_VALID_FOR_S} seconds"),
        ));
    }
    Ok(ReportForm {
        zone,
        severity,
        needs,
        who,
        valid_for_s,
    })
}

/// Draw until the zone matches: the draw also picks a real position inside the zone, and leaves
/// Part E's `campus.rs` as it is. About as many draws as zones, so a few; the cap only keeps this
/// from being an endless loop.
fn draw_in_zone(
    zone: ZoneId,
    zones: ZoneId,
    mut draw: impl FnMut() -> Incident,
) -> Option<Incident> {
    let tries = u32::from(zones.max(1)).saturating_mul(256);
    (0..tries)
        .map(|_| draw())
        .find(|incident| incident.zone == zone)
}

fn draw_incident(cfg: &CampusConfig, client: ClientId, who: u32, now_ms: u64) -> Incident {
    campus::unseeded_incident(cfg, UserId { client, user: who }, now_ms)
}

fn build_incident(app: &App, form: &ReportForm) -> Option<Incident> {
    let campus = &app.config.campus;
    let mut incident = draw_in_zone(form.zone, campus.zones, || {
        draw_incident(campus, app.client_id, form.who, now_ms())
    })?;
    incident.severity = form.severity;
    incident.needs = form.needs;
    // After `needs`, because the category must be one of that capability: the form asks for the
    // help needed, and `Capability::categories` lists what fits it.
    if let Some(category) = form.needs.categories().first() {
        incident.category = *category;
    }
    incident.ttl_ms = form.valid_for_s.saturating_mul(1000);
    Some(incident)
}

async fn report(app: &App, query: &Query<'_>) -> Result<Response, Response> {
    let form = parse_report(query, &app.config.campus)?;
    let Some(incident) = build_incident(app, &form) else {
        return Err(Response::error(500, "could not draw a place in that zone"));
    };
    // One click is one submit: the middleware copies it to the nodes and retries it. Never resent
    // from here, because a new ID would be a second incident.
    match app.client.submit(incident).await {
        Ok(done) => Ok(Response::json(
            200,
            &report_json(&done, &form, &app.responders),
        )),
        Err(ClientError::GaveUp { attempts }) => Ok(Response::json(
            504,
            &json!({
                "ok": false,
                "gave_up": true,
                "error": format!("No answer after {attempts} attempts; it may still be recorded later."),
            }),
        )),
        Err(e) => Ok(Response::error(502, format!("The report failed: {e}"))),
    }
}

fn responder_fields(
    responder: Option<ResponderId>,
    responders: &BTreeMap<ResponderId, Responder>,
) -> (Option<ZoneId>, Option<String>) {
    let known = responder.and_then(|id| responders.get(&id));
    (
        known.map(|r| r.zone),
        known.map(|r| capability_name(&r.capability)),
    )
}

/// The card's data. `request`, `log_index` and `term` are null for a provisional answer, which has
/// none yet. The ID is text, because JavaScript rounds a number above 2^53.
fn report_json(
    done: &Submitted,
    form: &ReportForm,
    responders: &BTreeMap<ResponderId, Responder>,
) -> Value {
    let committed = done.committed.as_ref();
    let (responder_zone, responder_capability) = responder_fields(done.plan.responder, responders);
    json!({
        "ok": true,
        "request": committed.map(|c| c.request.to_string()),
        "answered_by": done.from,
        "planned_by": done.executor,
        "responder": done.plan.responder,
        "responder_zone": responder_zone,
        "responder_capability": responder_capability,
        "cost_m": done.plan.cost,
        "status": done.status.as_str(),
        "priority": done.plan.priority,
        "provisional": done.provisional,
        "expired": done.status == IncidentStatus::Expired,
        "log_index": committed.map(|c| c.log_index),
        "term": committed.map(|c| c.term),
        "latency_us": u64::try_from(done.latency.as_micros()).unwrap_or(u64::MAX),
        "attempts": done.attempts,
        "zone": form.zone,
        "severity": form.severity,
        "needs": capability_name(&form.needs),
        "who": form.who,
        "valid_for_s": form.valid_for_s,
    })
}

// ---------------------------------------------------------------------------------------------
// Node table
// ---------------------------------------------------------------------------------------------

/// Ask every node at once, so a dead node costs one poll 800 ms, not 800 ms per node.
async fn status_rows(app: &App) -> Vec<Value> {
    let asks = app.config.cluster.nodes.iter().map(|node| async move {
        let answer = control(&node.client_addr, ControlRequest::Status, STATUS_TIMEOUT).await;
        (node.id, answer)
    });
    join_all(asks)
        .await
        .into_iter()
        .map(|(id, answer)| match answer {
            Ok(ControlReply::Status(status)) => node_json(&status),
            Ok(other) => json!({
                "id": id,
                "reachable": false,
                "error": format!("unexpected reply {other:?}"),
            }),
            Err(e) => json!({ "id": id, "reachable": false, "error": e.to_string() }),
        })
        .collect()
}

/// The digest is text, as `ctl` prints it, because JavaScript rounds a number above 2^53 and two
/// different digests could then look equal.
fn node_json(status: &NodeStatus) -> Value {
    json!({
        "id": status.id,
        "reachable": true,
        "role": status.role,
        "term": status.term,
        "leader": status.leader,
        "last_log_index": status.last_log_index,
        "last_applied": status.last_applied,
        "digest": format!("{:016x}", status.digest),
        "results": status.results,
        "responders": status.responders,
        "silenced": status.silenced,
        "serving": status.serving,
        "degraded": status.degraded,
        "members": status.members,
    })
}

async fn status(app: &App, query: &Query<'_>) -> Result<Response, Response> {
    query.only(&[])?;
    let nodes = status_rows(app).await;
    Ok(Response::json(200, &json!({ "ok": true, "nodes": nodes })))
}

// ---------------------------------------------------------------------------------------------
// Load box
// ---------------------------------------------------------------------------------------------

/// Noor's item 4. All three keep the same average rate: T between a user's reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pattern {
    /// Everyone waits exactly T, which is what the experiments use.
    Steady,
    /// Each user waits a random time from 0 to 2T.
    Random,
    /// Every user sends at the same moment, for spikes.
    AllAtOnce,
}

impl Pattern {
    const ALL: [Pattern; 3] = [Pattern::Steady, Pattern::Random, Pattern::AllAtOnce];

    /// The name on this page's API and on `loadgen --pattern`.
    fn name(self) -> &'static str {
        match self {
            Pattern::Steady => "steady",
            Pattern::Random => "random",
            Pattern::AllAtOnce => "all-at-once",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Pattern::Steady => "Steady",
            Pattern::Random => "Random",
            Pattern::AllAtOnce => "All at once",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LoadRequest {
    users: u32,
    think_ms: u64,
    duration_s: u64,
    pattern: Pattern,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgramState {
    Running,
    /// Stop was pressed and the program has not ended yet.
    Stopping,
    Finished,
    Stopped,
    Failed,
}

impl ProgramState {
    fn name(self) -> &'static str {
        match self {
            ProgramState::Running => "running",
            ProgramState::Stopping => "stopping",
            ProgramState::Finished => "finished",
            ProgramState::Stopped => "stopped",
            ProgramState::Failed => "failed",
        }
    }

    fn is_live(self) -> bool {
        matches!(self, ProgramState::Running | ProgramState::Stopping)
    }
}

struct ProgramRow {
    client_id: ClientId,
    pid: Option<u32>,
    users: u32,
    think_ms: u64,
    pattern: Pattern,
    state: ProgramState,
    exit_code: Option<i32>,
    summary: Option<String>,
    stop: Option<oneshot::Sender<()>>,
}

/// Every program started from this page. Held in a std mutex: sections are short and never cross
/// an `.await`.
#[derive(Default)]
struct LoadBox {
    rows: Vec<ProgramRow>,
    /// The last number given to a program. Never reused while this program runs.
    last_number: u64,
    tasks: JoinSet<()>,
}

/// Up to [`PROGRAMS_PER_CLICK`] programs share the users, and the first ones take the remainder:
/// 1000 = 10 x 100, 7 = 7 x 1, 1005 = 5 x 101 + 5 x 100.
fn split_users(users: u32) -> Vec<u32> {
    let programs = users.min(PROGRAMS_PER_CLICK);
    (0..programs)
        .map(|i| users / programs + u32::from(i < users % programs))
        .collect()
}

impl LoadBox {
    fn live(&self) -> impl Iterator<Item = &ProgramRow> {
        self.rows.iter().filter(|row| row.state.is_live())
    }

    /// The limits count what is already running, so clicking again cannot go around them.
    fn check_limits(&self, request: &LoadRequest) -> Result<(), String> {
        if request.users == 0 {
            return Err("how many user processes must be at least 1".to_string());
        }
        if request.think_ms < MIN_THINK_MS {
            return Err(format!(
                "the wait between reports must be at least {MIN_THINK_MS} ms"
            ));
        }
        if request.duration_s == 0 || request.duration_s > MAX_DURATION_S {
            return Err(format!("duration must be 1 to {MAX_DURATION_S} s"));
        }
        let running_users: u64 = self.live().map(|row| u64::from(row.users)).sum();
        let users = running_users + u64::from(request.users);
        if users > u64::from(MAX_RUNNING_USERS) {
            return Err(format!(
                "{users} users would be running on this PC; the limit is {MAX_RUNNING_USERS}"
            ));
        }
        let programs = split_users(request.users).len();
        let running_programs = self.live().count() + programs;
        if running_programs > MAX_RUNNING_PROGRAMS {
            return Err(format!(
                "{running_programs} programs would be running; the limit is {MAX_RUNNING_PROGRAMS}"
            ));
        }
        let running_rate: u64 = self
            .live()
            .map(|row| u64::from(row.users) * 1000 / row.think_ms.max(1))
            .sum();
        let rate = running_rate + u64::from(request.users) * 1000 / request.think_ms;
        if rate > MAX_OFFERED_REPORTS_PER_S {
            return Err(format!(
                "{rate} reports a second would be offered; the limit is {MAX_OFFERED_REPORTS_PER_S}"
            ));
        }
        if self.last_number + programs as u64 > MAX_PROGRAM_NUMBER {
            return Err("out of program numbers: restart the frontend".to_string());
        }
        Ok(())
    }

    /// Fire every stop. A program that has ended took its sender along.
    fn stop_all(&mut self) -> usize {
        let mut stopping = 0;
        for row in &mut self.rows {
            if let Some(stop) = row.stop.take() {
                let _ = stop.send(());
                row.state = ProgramState::Stopping;
                stopping += 1;
            }
        }
        stopping
    }

    /// Keep the list bounded, oldest ended rows first.
    fn forget_old_rows(&mut self) {
        let mut extra = self.rows.len().saturating_sub(MAX_PROGRAM_ROWS);
        self.rows.retain(|row| {
            if extra > 0 && !row.state.is_live() {
                extra -= 1;
                false
            } else {
                true
            }
        });
    }

    fn to_json(&self) -> Value {
        let rows: Vec<Value> = self
            .rows
            .iter()
            .map(|row| {
                json!({
                    "client_id": row.client_id,
                    "pid": row.pid,
                    "users": row.users,
                    "think_ms": row.think_ms,
                    "pattern": row.pattern.name(),
                    "state": row.state.name(),
                    "exit_code": row.exit_code,
                    "summary": row.summary,
                })
            })
            .collect();
        json!({
            "ok": true,
            "running_users": self.live().map(|row| u64::from(row.users)).sum::<u64>(),
            "running_programs": self.live().count(),
            "rows": rows,
        })
    }
}

fn load_start(app: &App, query: &Query<'_>) -> Result<Response, Response> {
    query.only(&["users", "think_ms", "duration_s", "pattern"])?;
    let pattern = query.get("pattern").unwrap_or(Pattern::Steady.name());
    let pattern = Pattern::parse(pattern)
        .ok_or_else(|| Response::error(400, format!("unknown pattern {pattern}")))?;
    let request = LoadRequest {
        users: query.number("users")?.unwrap_or(DEFAULT_USERS),
        think_ms: query.number("think_ms")?.unwrap_or(DEFAULT_THINK_MS),
        duration_s: query.number("duration_s")?.unwrap_or(DEFAULT_DURATION_S),
        pattern,
    };

    let mut load = lock(&app.load);
    load.check_limits(&request)
        .map_err(|why| Response::error(400, why))?;
    while load.tasks.try_join_next().is_some() {}
    let (mut started, mut failed) = (0u32, 0u32);
    for users in split_users(request.users) {
        load.last_number += 1;
        let client_id = app.client_id * PROGRAM_ID_BASE + load.last_number;
        if start_program(app, &mut load, client_id, users, &request) {
            started += 1;
        } else {
            failed += 1;
        }
    }
    load.forget_old_rows();
    tracing::info!(started, failed, users = request.users, "load box started");
    Ok(Response::json(
        if started > 0 { 200 } else { 500 },
        &json!({
            "ok": started > 0,
            "started": started,
            "failed": failed,
            "error": (started == 0).then(|| format!("could not start {}", app.loadgen.display())),
        }),
    ))
}

/// Start one real program and give it a task that owns it. False if it could not start, which
/// leaves a failed row: a restart would hide the failure.
fn start_program(
    app: &App,
    load: &mut LoadBox,
    client_id: ClientId,
    users: u32,
    request: &LoadRequest,
) -> bool {
    let mut command = program_command(&app.loadgen, &app.config_path, client_id, users, request);
    let mut row = ProgramRow {
        client_id,
        pid: None,
        users,
        think_ms: request.think_ms,
        pattern: request.pattern,
        state: ProgramState::Running,
        exit_code: None,
        summary: None,
        stop: None,
    };
    match command.spawn() {
        Ok(child) => {
            let (stop, stopped) = oneshot::channel();
            row.pid = child.id();
            row.stop = Some(stop);
            load.rows.push(row);
            load.tasks
                .spawn(watch_program(app.load.clone(), client_id, child, stopped));
            true
        }
        Err(e) => {
            row.state = ProgramState::Failed;
            row.summary = Some(format!("could not start {}: {e}", app.loadgen.display()));
            load.rows.push(row);
            false
        }
    }
}

fn program_command(
    loadgen: &Path,
    config_path: &Path,
    client_id: ClientId,
    users: u32,
    request: &LoadRequest,
) -> Command {
    let mut command = Command::new(loadgen);
    command
        .arg("--config")
        .arg(config_path)
        .arg("--client-id")
        .arg(client_id.to_string())
        .arg("--users")
        .arg(users.to_string())
        .arg("--think-ms")
        .arg(request.think_ms.to_string())
        .arg("--duration-s")
        .arg(request.duration_s.to_string())
        // One seed per program: a shared seed would make their users report the same incidents.
        .arg("--seed")
        .arg(client_id.to_string())
        .arg("--pattern")
        .arg(request.pattern.name())
        // Otherwise each of up to ten programs starts one runtime thread per core, as in
        // scripts/start-users.sh.
        .env("TOKIO_WORKER_THREADS", worker_threads(request.users))
        .stdin(Stdio::null())
        // Read to the end by wait_with_output, so a program can never block on a full pipe.
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        // A program must not outlive the page that shows it.
        .kill_on_drop(true);
    command
}

fn worker_threads(users: u32) -> String {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    (cores / split_users(users).len().max(1)).max(1).to_string()
}

/// Owns one program until it ends, and writes down how it ended.
async fn watch_program(
    load: Arc<Mutex<LoadBox>>,
    client_id: ClientId,
    child: Child,
    mut stop: oneshot::Receiver<()>,
) {
    let pid = child.id();
    let mut output = Box::pin(child.wait_with_output());
    let ended = tokio::select! {
        ended = &mut output => Some(ended),
        // Only Ok(()): a dropped sender must never kill a program.
        Ok(()) = &mut stop => None,
    };
    let (ended, stopped) = match ended {
        Some(ended) => (Some(ended), false),
        None => {
            ask_to_end(pid).await;
            (timeout(STOP_GRACE, &mut output).await.ok(), true)
        }
    };
    // Drops the child with the future: kill_on_drop ends one that ignored the request.
    drop(output);
    record_end(&load, client_id, ended, stopped);
}

/// `kill -TERM`, by process ID. tokio only offers SIGKILL, unsafe is forbidden and libc is not a
/// direct dependency. loadgen prints its summary before it goes (D183).
async fn ask_to_end(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    let term = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .status();
    match timeout(TERM_TIMEOUT, term).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => tracing::warn!("cannot run kill: {e}"),
        Err(_) => tracing::warn!("kill did not finish in time"),
    }
}

fn summary_text(stdout: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(&stdout[..stdout.len().min(MAX_SUMMARY_BYTES)]);
    let text = text.trim_end();
    (!text.is_empty()).then(|| text.to_string())
}

fn record_end(
    load: &Mutex<LoadBox>,
    client_id: ClientId,
    ended: Option<std::io::Result<Output>>,
    stopped: bool,
) {
    let (state, exit_code, summary) = match ended {
        Some(Ok(output)) => {
            let state = if stopped {
                ProgramState::Stopped
            } else if output.status.success() {
                ProgramState::Finished
            } else {
                ProgramState::Failed
            };
            (state, output.status.code(), summary_text(&output.stdout))
        }
        Some(Err(e)) => (
            if stopped {
                ProgramState::Stopped
            } else {
                ProgramState::Failed
            },
            None,
            Some(format!("could not read the program: {e}")),
        ),
        None => (ProgramState::Stopped, None, None),
    };
    let mut load = lock(load);
    if let Some(row) = load.rows.iter_mut().find(|row| row.client_id == client_id) {
        row.state = state;
        row.exit_code = exit_code;
        row.summary = summary;
        row.stop = None;
    }
}

fn load_stop(app: &App) -> Response {
    let stopping = lock(&app.load).stop_all();
    tracing::info!(stopping, "load box stopping");
    Response::json(200, &json!({ "ok": true, "stopping": stopping }))
}

#[cfg(test)]
#[path = "../../tests/common/mod.rs"]
mod common;

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::OnceLock;

    use rescuemesh::config::AssignmentPolicy;
    use rescuemesh::proto::{Category, CommittedResult, Plan, Point, RequestId};
    use tokio::io::AsyncWriteExt;
    use tokio::task::JoinHandle;
    use tokio::time::Instant;

    use super::common::{TestCluster, wait_until};
    use super::*;

    const PATIENCE: Duration = Duration::from_secs(15);

    /// The real server on a real socket, in this process.
    struct Running {
        addr: SocketAddr,
        app: Arc<App>,
        stop: oneshot::Sender<()>,
        server: JoinHandle<()>,
    }

    impl Running {
        async fn end(self) {
            let _ = self.stop.send(());
            self.server.await.unwrap();
            self.app.shutdown().await;
        }
    }

    async fn start_page(config: Config, loadgen: PathBuf) -> Running {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Arc::new(App::new(
            config,
            PathBuf::from("the-config.toml"),
            11,
            EventLog::disabled("frontend-11"),
            loadgen,
        ));
        let (stop, stopped) = oneshot::channel::<()>();
        let server = tokio::spawn(serve(listener, app.clone(), async move {
            let _ = stopped.await;
        }));
        Running {
            addr,
            app,
            stop,
            server,
        }
    }

    /// A browser in a few lines: what it sends is what the tests assert about, byte by byte.
    async fn raw_http(addr: SocketAddr, request: &str) -> (u16, String, String) {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).await.unwrap();
        let answer = String::from_utf8(answer).unwrap();
        let (head, body) = answer.split_once("\r\n\r\n").unwrap();
        let status = head.split(' ').nth(1).unwrap().parse().unwrap();
        (status, head.to_string(), body.to_string())
    }

    async fn http(addr: SocketAddr, method: &str, target: &str) -> (u16, Value) {
        let request =
            format!("{method} {target} HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n");
        let (status, _, body) = raw_http(addr, &request).await;
        (status, serde_json::from_str(&body).unwrap())
    }

    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    /// Three addresses nobody listens on, and retries short enough for a test.
    fn dead_config() -> Config {
        let mut text = String::new();
        for id in 1..=3 {
            text.push_str(&format!(
                "[[cluster.nodes]]\nid = {id}\npeer_addr = \"127.0.0.1:{}\"\nclient_addr = \"127.0.0.1:{}\"\n",
                free_port(),
                free_port()
            ));
        }
        text.push_str(
            "[timers]\nclient_retry_ms = 50\nclient_retry_max_ms = 100\nclient_max_retries = 1\n",
        );
        Config::from_toml(&text).unwrap()
    }

    fn no_program() -> PathBuf {
        PathBuf::from("/nonexistent/loadgen")
    }

    #[test]
    fn users_split_over_at_most_ten_programs() {
        assert_eq!(split_users(1000), vec![100; 10]);
        assert_eq!(split_users(7), vec![1; 7]);
        let mut expected = vec![101; 5];
        expected.extend([100; 5]);
        assert_eq!(split_users(1005), expected);
        assert_eq!(split_users(10), vec![1; 10]);
        let mut eleven = vec![1; 10];
        eleven[0] = 2;
        assert_eq!(split_users(11), eleven);
        assert_eq!(split_users(1), vec![1]);
        assert!(split_users(0).is_empty());
    }

    #[tokio::test]
    async fn request_parser_handles_split_reads_and_caps() {
        let (mut writer, mut reader) = tokio::io::duplex(16);
        let sender = tokio::spawn(async move {
            for piece in [
                "POST /api/rep",
                "ort?zone=2&severity=4 HT",
                "TP/1.1\r\nHost: x\r",
                "\n\r\n",
            ] {
                writer.write_all(piece.as_bytes()).await.unwrap();
                tokio::task::yield_now().await;
            }
            writer
        });
        let head = read_head(&mut reader).await.unwrap();
        let _writer = sender.await.unwrap();
        let request = parse_head(&head).unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/api/report");
        assert_eq!(
            request.query,
            vec![
                ("zone".to_string(), "2".to_string()),
                ("severity".to_string(), "4".to_string())
            ]
        );

        // No blank line in sight: refused at the cap, without waiting for more.
        let (mut writer, mut reader) = tokio::io::duplex(MAX_HEAD_BYTES * 2);
        writer
            .write_all(&vec![b'a'; MAX_HEAD_BYTES + 1])
            .await
            .unwrap();
        assert_eq!(read_head(&mut reader).await, Err(HeadError::TooLarge));

        // A blank line past the cap is refused too.
        let (mut writer, mut reader) = tokio::io::duplex(MAX_HEAD_BYTES * 2);
        let mut big = vec![b'a'; MAX_HEAD_BYTES];
        big.extend_from_slice(b"\r\n\r\n");
        writer.write_all(&big).await.unwrap();
        assert_eq!(read_head(&mut reader).await, Err(HeadError::TooLarge));

        // The browser went away half way.
        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer.write_all(b"GET / HTTP").await.unwrap();
        drop(writer);
        assert_eq!(read_head(&mut reader).await, Err(HeadError::Closed));
    }

    #[test]
    fn bad_request_heads_are_refused() {
        for head in [
            "GET",
            "GET /",
            "GET / HTTP/1.1 extra",
            "GET / HTTP/2",
            "GET / HTTP/1.1\r\nno colon here",
            "POST /api/report HTTP/1.1\r\nContent-Length: 5",
            "POST /api/report HTTP/1.1\r\nTransfer-Encoding: chunked",
        ] {
            assert!(
                matches!(parse_head(head.as_bytes()), Err(HeadError::Malformed(_))),
                "{head}"
            );
        }
        assert!(parse_head(&[0xff, 0xfe]).is_err());
        assert!(parse_head(b"POST / HTTP/1.1\r\nContent-Length: 0").is_ok());
    }

    #[test]
    fn a_drawn_incident_lands_in_the_asked_zone() {
        let campus = CampusConfig::default();
        for zone in 0..campus.zones {
            let incident =
                draw_in_zone(zone, campus.zones, || draw_incident(&campus, 11, 1, 5)).unwrap();
            assert_eq!(incident.zone, zone);
        }
        // A zone that can never come up ends the loop instead of hanging the page.
        assert!(draw_in_zone(9, 6, || draw_incident(&campus, 11, 1, 5)).is_none());
    }

    fn submitted(committed: Option<CommittedResult>, plan: Plan) -> Submitted {
        let status = match (&committed, plan.responder) {
            (None, _) => IncidentStatus::Provisional,
            (Some(_), Some(_)) => IncidentStatus::Assigned,
            (Some(_), None) => IncidentStatus::Unassigned,
        };
        Submitted {
            id: RequestId {
                client: 11,
                seq: u64::MAX - 1,
            },
            from: 2,
            executor: 3,
            plan,
            provisional: committed.is_none(),
            committed,
            status,
            latency: Duration::from_millis(30),
            attempts: 1,
        }
    }

    fn form() -> ReportForm {
        ReportForm {
            zone: 2,
            severity: 4,
            needs: Capability::Medical,
            who: 7,
            valid_for_s: 60,
        }
    }

    fn incident() -> Incident {
        Incident {
            owner: UserId {
                client: 11,
                user: 7,
            },
            category: Category::MedicalAssistance,
            zone: 2,
            location: Point { x: 10, y: 20 },
            severity: 4,
            needs: Capability::Medical,
            reported_at_ms: 5,
            ttl_ms: 60_000,
        }
    }

    fn committed(plan: Plan) -> CommittedResult {
        CommittedResult {
            // Far above 2^53: as a JSON number it would come out rounded in the browser.
            request: RequestId {
                client: 11,
                seq: u64::MAX - 1,
            },
            incident: incident(),
            executor: 3,
            term: 8,
            plan,
            age_ms: 0,
            assigned_at_ms: 6,
            log_index: 41,
        }
    }

    fn some_responders() -> BTreeMap<ResponderId, Responder> {
        campus::responders(&CampusConfig::default())
            .into_iter()
            .map(|r| (r.id, r))
            .collect()
    }

    fn plan() -> Plan {
        Plan {
            responder: Some(9),
            cost: 340,
            priority: 460,
        }
    }

    #[test]
    fn provisional_json_has_no_index_or_request() {
        let done = submitted(None, plan());
        let json = report_json(&done, &form(), &some_responders());
        assert_eq!(json["provisional"], true);
        assert_eq!(json["status"], "provisional");
        assert_eq!(json["expired"], false);
        for absent in ["request", "log_index", "term"] {
            assert!(json[absent].is_null(), "{absent}");
        }
        assert_eq!(json["responder"], 9);
        assert_eq!(json["answered_by"], 2);
        assert_eq!(json["planned_by"], 3);
    }

    #[test]
    fn committed_json_names_everything_and_the_id_is_text() {
        let done = submitted(Some(committed(plan())), plan());
        let json = report_json(&done, &form(), &some_responders());
        assert_eq!(json["provisional"], false);
        assert_eq!(json["status"], "assigned");
        assert_eq!(json["request"], format!("11-{}", u64::MAX - 1));
        assert_eq!(json["log_index"], 41);
        assert_eq!(json["term"], 8);
        assert_eq!(json["cost_m"], 340);
        assert_eq!(json["priority"], 460);
        assert_eq!(json["latency_us"], 30_000);
        let responder = some_responders()[&9].clone();
        assert_eq!(json["responder_zone"], responder.zone);
        assert_eq!(
            json["responder_capability"],
            capability_name(&responder.capability)
        );
    }

    #[test]
    fn nobody_free_and_an_expired_answer_are_told_apart() {
        let nobody = Plan {
            responder: None,
            cost: 0,
            priority: 0,
        };
        let done = submitted(Some(committed(nobody.clone())), nobody);
        let free = report_json(&done, &form(), &some_responders());
        assert_eq!(free["status"], "unassigned");
        assert_eq!(free["expired"], false);
        assert!(free["responder"].is_null());
        let refused = Submitted {
            status: IncidentStatus::Expired,
            ..done
        };
        let late = report_json(&refused, &form(), &some_responders());
        assert_eq!(late["expired"], true);
        assert_eq!(late["status"], "expired");
    }

    #[test]
    fn digest_is_text_and_survives_above_two_to_the_53() {
        let status = NodeStatus {
            id: 1,
            term: 3,
            leader: Some(1),
            role: "Leader".to_string(),
            last_log_index: Some(9),
            last_applied: None,
            digest: 0xfedc_ba98_7654_3211,
            results: 2,
            responders: 30,
            silenced: false,
            serving: true,
            degraded: false,
            members: vec![1, 3],
        };
        let json = node_json(&status);
        assert_eq!(json["digest"], "fedcba9876543211");
        assert!(json["last_applied"].is_null());
        assert_eq!(json["reachable"], true);
        assert_eq!(json["members"], json!([1, 3]));

        let small = NodeStatus {
            digest: 255,
            ..status
        };
        assert_eq!(node_json(&small)["digest"], "00000000000000ff");
    }

    fn running_row(users: u32, think_ms: u64) -> ProgramRow {
        ProgramRow {
            client_id: 110_001,
            pid: Some(1),
            users,
            think_ms,
            pattern: Pattern::Steady,
            state: ProgramState::Running,
            exit_code: None,
            summary: None,
            stop: None,
        }
    }

    fn ask(users: u32, think_ms: u64, duration_s: u64) -> LoadRequest {
        LoadRequest {
            users,
            think_ms,
            duration_s,
            pattern: Pattern::Steady,
        }
    }

    #[test]
    fn load_limits_are_refused() {
        let empty = LoadBox::default();
        // Noor's example, and the longest run, are both inside the limits.
        assert!(empty.check_limits(&ask(1000, 1000, 900)).is_ok());
        assert!(empty.check_limits(&ask(1, 200, 1)).is_ok());
        for refused in [
            ask(0, 1000, 60),
            ask(1001, 1000, 60),
            ask(10, 199, 60),
            ask(10, 1000, 0),
            ask(10, 1000, 901),
            // 400 users at 200 ms offer 2000 reports a second.
            ask(400, 200, 60),
        ] {
            assert!(empty.check_limits(&refused).is_err(), "{refused:?}");
        }

        // What is already running counts.
        let mut busy = LoadBox::default();
        busy.rows.push(running_row(995, 1000));
        assert!(busy.check_limits(&ask(5, 1000, 60)).is_ok());
        let too_many = busy.check_limits(&ask(6, 1000, 60)).unwrap_err();
        assert!(too_many.contains("1001"), "{too_many}");

        let mut fast = LoadBox::default();
        fast.rows.push(running_row(290, 200));
        assert!(fast.check_limits(&ask(10, 200, 60)).is_ok());
        let too_fast = fast.check_limits(&ask(11, 200, 60)).unwrap_err();
        assert!(too_fast.contains("reports a second"), "{too_fast}");

        // The 21st program.
        let mut many = LoadBox::default();
        many.rows.extend((0..20).map(|_| running_row(1, 1000)));
        let too_many = many.check_limits(&ask(1, 1000, 60)).unwrap_err();
        assert!(too_many.contains("21"), "{too_many}");
        // Ended programs count for nothing.
        for row in &mut many.rows {
            row.state = ProgramState::Finished;
        }
        assert!(many.check_limits(&ask(1, 1000, 60)).is_ok());

        let mut numbered = LoadBox {
            last_number: MAX_PROGRAM_NUMBER - 1,
            ..LoadBox::default()
        };
        assert!(numbered.check_limits(&ask(1, 1000, 60)).is_ok());
        numbered.last_number = MAX_PROGRAM_NUMBER;
        assert!(numbered.check_limits(&ask(1, 1000, 60)).is_err());
    }

    #[test]
    fn old_rows_go_first_and_running_rows_stay() {
        let mut load = LoadBox::default();
        load.rows.push(running_row(1, 1000));
        for _ in 0..MAX_PROGRAM_ROWS + 5 {
            let mut row = running_row(1, 1000);
            row.state = ProgramState::Finished;
            load.rows.push(row);
        }
        load.forget_old_rows();
        assert_eq!(load.rows.len(), MAX_PROGRAM_ROWS);
        assert!(load.rows[0].state.is_live());
    }

    #[test]
    fn the_page_has_no_outside_links_and_writes_text_only() {
        for banned in [
            "innerHTML",
            "outerHTML",
            "insertAdjacentHTML",
            "document.write",
            "eval(",
            "http://",
            "https://",
            "<link",
            "<iframe",
            "<img",
            "src=",
            "href=",
            "@import",
            "url(",
        ] {
            assert!(!PAGE.contains(banned), "the page must not use {banned}");
        }
        for needed in [
            "report-panel",
            "load-panel",
            "nodes-panel",
            "PROVISIONAL",
            "EXPIRED",
            "How many user processes",
            "Who sent it",
            "Valid for",
            "unreachable",
            "Local program not answering",
        ] {
            assert!(PAGE.contains(needed), "the page must have {needed}");
        }
        // Publish-subscribe is Phase 2, and every API the page uses now exists.
        for gone in ["zones-panel", "/api/zones", "not available yet"] {
            assert!(!PAGE.contains(gone), "the page must not have {gone}");
        }
    }

    /// Every call in the page's JavaScript goes to a real route with the right method, and every
    /// route is used. A typo here would otherwise only show in a browser.
    #[test]
    fn the_page_calls_exactly_the_routes_that_exist() {
        let mut called = BTreeSet::new();
        for piece in PAGE.split("call(\"").skip(1) {
            let mut fields = piece.splitn(3, '"');
            let method = fields.next().unwrap();
            let _separator = fields.next().unwrap();
            let path = fields.next().unwrap().split('"').next().unwrap();
            assert!(
                ROUTES.contains(&(path, method)),
                "the page calls {method} {path}, which does not exist"
            );
            called.insert(path.to_string());
        }
        for (path, _) in ROUTES {
            assert!(
                *path == "/" || called.contains(*path),
                "nothing calls {path}"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_page_and_info_are_served_as_a_browser_needs() {
        let page = start_page(dead_config(), no_program()).await;
        let (status, head, body) =
            raw_http(page.addr, "GET / HTTP/1.1\r\nHost: test\r\n\r\n").await;
        assert_eq!(status, 200);
        assert_eq!(body, PAGE);
        for line in [
            "Content-Type: text/html; charset=utf-8".to_string(),
            format!("Content-Length: {}", PAGE.len()),
            "Connection: close".to_string(),
            "Cache-Control: no-store".to_string(),
            "Content-Security-Policy: default-src 'none'".to_string(),
        ] {
            assert!(head.contains(&line), "{line} is missing from {head}");
        }

        let (status, info) = http(page.addr, "GET", "/api/info").await;
        assert_eq!(status, 200);
        assert_eq!(info["client_id"], 11);
        assert_eq!(info["zones"], 6);
        assert_eq!(info["severities"].as_array().unwrap().len(), 5);
        assert_eq!(
            info["capabilities"].as_array().unwrap().len(),
            Capability::ALL.len()
        );
        let patterns: Vec<&str> = info["patterns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(patterns, ["steady", "random", "all-at-once"]);
        page.end().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_node_parameter_and_bad_values_are_refused() {
        let page = start_page(dead_config(), no_program()).await;
        for target in [
            "/api/report?zone=1&severity=3&needs=Medical&node=2",
            "/api/report?zone=6&severity=3&needs=Medical",
            "/api/report?zone=-1&severity=3&needs=Medical",
            "/api/report?severity=3&needs=Medical",
            "/api/report?zone=1&severity=0&needs=Medical",
            "/api/report?zone=1&severity=6&needs=Medical",
            "/api/report?zone=1&needs=Medical",
            "/api/report?zone=1&severity=3&needs=Wizard",
            "/api/report?zone=1&severity=3",
            "/api/report?zone=1&zone=2&severity=3&needs=Medical",
            "/api/report?zone=1&severity=3&needs=Medical&who=abc",
            "/api/report?zone=1&severity=3&needs=Medical&valid_for_s=0",
            "/api/report?zone=1&severity=3&needs=Medical&valid_for_s=3601",
            "/api/load/start?users=10&node=2",
            "/api/load/start?users=lots",
            "/api/load/start?users=10&pattern=wave",
            "/api/load/stop?now=1",
        ] {
            let (status, answer) = http(page.addr, "POST", target).await;
            assert_eq!(status, 400, "{target}: {answer}");
            assert_eq!(answer["ok"], false, "{target}");
            assert!(answer["error"].is_string(), "{target}");
        }
        assert_eq!(http(page.addr, "GET", "/?x=1").await.0, 400);
        assert_eq!(http(page.addr, "GET", "/nope").await.0, 404);
        assert_eq!(http(page.addr, "GET", "/api/zones").await.0, 404);
        assert_eq!(http(page.addr, "GET", "/api/report").await.0, 405);
        assert_eq!(http(page.addr, "POST", "/api/status").await.0, 405);
        let (status, head, _) = raw_http(
            page.addr,
            "DELETE /api/load/stop HTTP/1.1\r\nHost: t\r\n\r\n",
        )
        .await;
        assert_eq!(status, 405);
        assert!(head.contains("Allow: POST"), "{head}");
        // No bodies: values go in the query string.
        let (status, _, _) = raw_http(
            page.addr,
            "POST /api/load/stop HTTP/1.1\r\nHost: t\r\nContent-Length: 3\r\n\r\nabc",
        )
        .await;
        assert_eq!(status, 400);
        let (status, _, _) = raw_http(page.addr, "nonsense\r\n\r\n").await;
        assert_eq!(status, 400);
        let big = format!(
            "GET / HTTP/1.1\r\nX: {}\r\n\r\n",
            "y".repeat(MAX_HEAD_BYTES)
        );
        assert_eq!(raw_http(page.addr, &big).await.0, 431);
        // Nothing was started by any of that.
        assert_eq!(lock(&page.app.load).rows.len(), 0);
        page.end().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn with_no_node_left_a_report_ends_in_no_answer_and_rows_say_why() {
        let page = start_page(dead_config(), no_program()).await;
        let (status, answer) = http(
            page.addr,
            "POST",
            "/api/report?zone=1&severity=3&needs=Medical",
        )
        .await;
        assert_eq!(status, 504);
        assert_eq!(answer["ok"], false);
        assert_eq!(answer["gave_up"], true);
        let error = answer["error"].as_str().unwrap();
        assert!(error.contains("No answer after 2 attempts"), "{error}");
        assert!(error.contains("may still be recorded later"), "{error}");

        let (status, nodes) = http(page.addr, "GET", "/api/status").await;
        assert_eq!(status, 200);
        let nodes = nodes["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 3);
        for (i, node) in nodes.iter().enumerate() {
            assert_eq!(node["id"], i + 1);
            assert_eq!(node["reachable"], false);
            assert!(node["error"].is_string());
        }
        page.end().await;
    }

    /// The page's names are the ones `loadgen --pattern` takes (D182). `tests/frontend.rs` runs a
    /// real loadgen with them.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn every_pattern_reaches_the_program() {
        let page = start_page(dead_config(), fake_loadgen()).await;
        for pattern in ["random", "all-at-once"] {
            let target = format!("/api/load/start?users=1&duration_s=1&pattern={pattern}");
            let (status, answer) = http(page.addr, "POST", &target).await;
            assert_eq!(status, 200, "{answer}");
        }
        let rows = wait_for_rows(page.addr, "two finished programs", |rows| {
            rows.len() == 2 && rows.iter().all(|r| r["state"] == "finished")
        })
        .await;
        for (row, pattern) in rows.iter().zip(["random", "all-at-once"]) {
            assert_eq!(row["pattern"], pattern);
            let summary = row["summary"].as_str().unwrap();
            assert!(
                summary.ends_with(&format!("--pattern {pattern}")),
                "{summary}"
            );
        }
        page.end().await;
    }

    async fn ready(cluster: &TestCluster) {
        wait_until(PATIENCE, "a leader and responders everywhere", || {
            cluster
                .nodes
                .iter()
                .all(|n| n.ctx.consensus.status().leader.is_some() && n.ctx.status().responders > 0)
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_typed_report_returns_a_committed_card() {
        let cluster = TestCluster::start(3).await;
        ready(&cluster).await;
        let page = start_page(cluster.config.clone(), no_program()).await;

        let (status, card) = http(
            page.addr,
            "POST",
            "/api/report?zone=2&severity=4&needs=Medical&who=7&valid_for_s=60",
        )
        .await;
        assert_eq!(status, 200, "{card}");
        assert_eq!(card["ok"], true);
        assert_eq!(card["provisional"], false);
        assert_eq!(card["status"], "assigned");
        assert_eq!(card["expired"], false);
        assert_eq!(card["attempts"], 1);
        assert_eq!(card["who"], 7);
        assert_eq!(card["valid_for_s"], 60);
        assert!(card["responder"].is_u64());
        assert!(card["log_index"].is_u64());
        assert!(card["term"].is_u64());
        assert!(card["priority"].as_u64().unwrap() > 0);

        // What was typed is what the cloud recorded.
        let request = card["request"].as_str().unwrap();
        let (client, seq) = request.split_once('-').unwrap();
        assert_eq!(client, "11");
        let id = RequestId {
            client: 11,
            seq: seq.parse().unwrap(),
        };
        let from = card["answered_by"].as_u64().unwrap();
        let stored = cluster
            .node(from)
            .ctx
            .state
            .with(|s| s.result(&id).cloned())
            .unwrap();
        assert_eq!(stored.incident.zone, 2);
        assert_eq!(stored.incident.severity, 4);
        assert_eq!(stored.incident.needs, Capability::Medical);
        assert_eq!(
            stored.incident.owner,
            UserId {
                client: 11,
                user: 7
            }
        );
        assert_eq!(stored.incident.ttl_ms, 60_000);
        assert!(
            Capability::Medical
                .categories()
                .contains(&stored.incident.category)
        );
        assert_eq!(
            card["priority"].as_u64(),
            Some(u64::from(stored.plan.priority))
        );
        assert_eq!(card["planned_by"].as_u64(), Some(stored.executor));
        assert_eq!(card["log_index"].as_u64(), Some(stored.log_index));

        // The real table, for the same cluster.
        let (status, nodes) = http(page.addr, "GET", "/api/status").await;
        assert_eq!(status, 200);
        for node in nodes["nodes"].as_array().unwrap() {
            assert_eq!(node["reachable"], true, "{node}");
            assert_eq!(node["silenced"], false);
            assert_eq!(node["digest"].as_str().unwrap().len(), 16);
            assert!(node["role"].is_string());
            assert!(node["term"].is_u64());
            assert!(node["serving"].is_boolean());
            assert!(node["degraded"].is_boolean());
        }
        page.end().await;
        cluster.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_lone_nodes_answer_is_labelled_provisional_and_silenced_is_not_unreachable() {
        let cluster = TestCluster::start_with(3, |c| {
            c.policy.assignment = AssignmentPolicy::Bidding;
            c.timers.degraded_after_ms = 300;
            c.timers.client_retry_ms = 300;
            c.timers.client_retry_max_ms = 600;
        })
        .await;
        ready(&cluster).await;
        let leader = cluster
            .nodes
            .iter()
            .find_map(|n| n.ctx.consensus.status().leader)
            .unwrap();
        let lone = (1..=3).find(|id| *id != leader).unwrap();
        for id in (1..=3).filter(|id| *id != lone) {
            cluster.node(id).ctx.gate.silence();
        }
        wait_until(PATIENCE, "the last node to go degraded", || {
            cluster.node(lone).ctx.status().degraded
        })
        .await;
        let page = start_page(cluster.config.clone(), no_program()).await;

        let (status, card) = http(
            page.addr,
            "POST",
            "/api/report?zone=0&severity=1&needs=Medical",
        )
        .await;
        assert_eq!(status, 200, "{card}");
        assert_eq!(card["ok"], true);
        assert_eq!(card["provisional"], true);
        assert_eq!(card["status"], "provisional");
        assert_eq!(card["answered_by"].as_u64(), Some(lone));
        assert_eq!(card["planned_by"].as_u64(), Some(lone));
        for absent in ["request", "log_index", "term"] {
            assert!(card[absent].is_null(), "{absent}: {card}");
        }

        // A silenced node still answers control frames, so it is silenced, not unreachable.
        let (_, nodes) = http(page.addr, "GET", "/api/status").await;
        for node in nodes["nodes"].as_array().unwrap() {
            let id = node["id"].as_u64().unwrap();
            assert_eq!(node["reachable"], true, "{node}");
            assert_eq!(node["silenced"], id != lone, "{node}");
            if id == lone {
                assert_eq!(node["degraded"], true, "{node}");
            }
        }
        page.end().await;
        for id in (1..=3).filter(|id| *id != lone) {
            cluster.node(id).ctx.gate.restore();
        }
        cluster.shutdown().await;
    }

    /// Stands in for loadgen: prints its arguments as its summary. `--duration-s 1` finishes,
    /// `--duration-s 2` fails with code 3, anything else stays until stopped.
    #[cfg(unix)]
    fn fake_loadgen() -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        // Written once, before any test here can fork: a fork during the write would keep the file
        // open for writing and make exec fail with "text file busy".
        static PATH: OnceLock<PathBuf> = OnceLock::new();
        PATH.get_or_init(|| {
            let path = std::env::temp_dir().join(format!("rm-fake-loadgen-{}", std::process::id()));
            let script = "#!/bin/sh\necho \"args: $*\"\ncase \"$*\" in\n  \
                 *\"--duration-s 1 \"*) exit 0 ;;\n  *\"--duration-s 2 \"*) exit 3 ;;\nesac\n\
                 exec sleep 60\n";
            std::fs::write(&path, script).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        })
        .clone()
    }

    async fn wait_for_rows(
        addr: SocketAddr,
        what: &str,
        ok: impl Fn(&[Value]) -> bool,
    ) -> Vec<Value> {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let (_, load) = http(addr, "GET", "/api/load").await;
            let rows = load["rows"].as_array().unwrap();
            if ok(rows) {
                return rows.clone();
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}: {load}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_box_starts_ten_real_programs_and_shows_each_summary() {
        let page = start_page(dead_config(), fake_loadgen()).await;
        let (status, answer) = http(
            page.addr,
            "POST",
            "/api/load/start?users=1000&think_ms=1000&duration_s=1&pattern=steady",
        )
        .await;
        assert_eq!(status, 200, "{answer}");
        assert_eq!(answer["started"], 10);
        let rows = wait_for_rows(page.addr, "ten finished programs", |rows| {
            rows.len() == 10 && rows.iter().all(|r| r["state"] == "finished")
        })
        .await;
        for (row, n) in rows.iter().zip(1u64..) {
            let id = 110_000 + n;
            assert_eq!(row["client_id"], id);
            assert_eq!(row["users"], 100);
            assert_eq!(row["pattern"], "steady");
            assert_eq!(row["exit_code"], 0);
            assert!(row["pid"].is_u64());
            let summary = row["summary"].as_str().unwrap();
            assert_eq!(
                summary,
                format!(
                    "args: --config the-config.toml --client-id {id} --users 100 --think-ms 1000 \
                     --duration-s 1 --seed {id} --pattern steady"
                )
            );
        }
        // Another click adds a batch with new numbers.
        http(page.addr, "POST", "/api/load/start?users=7&duration_s=2").await;
        let rows = wait_for_rows(page.addr, "a failed batch", |rows| {
            rows.len() == 17 && rows[10..].iter().all(|r| r["state"] == "failed")
        })
        .await;
        assert_eq!(rows[16]["client_id"], 110_017);
        assert_eq!(rows[16]["users"], 1);
        assert_eq!(rows[16]["exit_code"], 3);
        page.end().await;
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stop_ends_the_programs_and_marks_them_stopped() {
        let page = start_page(dead_config(), fake_loadgen()).await;
        let (status, answer) =
            http(page.addr, "POST", "/api/load/start?users=7&duration_s=600").await;
        assert_eq!(status, 200, "{answer}");
        assert_eq!(answer["started"], 7);
        let (_, load) = http(page.addr, "GET", "/api/load").await;
        assert_eq!(load["running_users"], 7);
        assert_eq!(load["running_programs"], 7);
        // More than the limit, counting what runs: refused while they run.
        let (status, _) = http(page.addr, "POST", "/api/load/start?users=994").await;
        assert_eq!(status, 400);
        let pids: Vec<u64> = load["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["pid"].as_u64().unwrap())
            .collect();
        assert_eq!(pids.len(), 7);

        let (status, answer) = http(page.addr, "POST", "/api/load/stop").await;
        assert_eq!(status, 200);
        assert_eq!(answer["stopping"], 7);
        wait_for_rows(page.addr, "seven stopped programs", |rows| {
            rows.iter().all(|r| r["state"] == "stopped")
        })
        .await;
        for pid in pids {
            assert!(
                !Path::new(&format!("/proc/{pid}")).exists(),
                "{pid} is alive"
            );
        }
        let (_, load) = http(page.addr, "GET", "/api/load").await;
        assert_eq!(load["running_users"], 0);
        // A second press has nothing left to do.
        let (_, again) = http(page.addr, "POST", "/api/load/stop").await;
        assert_eq!(again["stopping"], 0);
        page.end().await;
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn shutdown_ends_every_program() {
        let page = start_page(dead_config(), fake_loadgen()).await;
        http(page.addr, "POST", "/api/load/start?users=3&duration_s=600").await;
        let (_, load) = http(page.addr, "GET", "/api/load").await;
        let pids: Vec<u64> = load["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["pid"].as_u64().unwrap())
            .collect();
        assert_eq!(pids.len(), 3);
        page.end().await;
        for pid in pids {
            assert!(
                !Path::new(&format!("/proc/{pid}")).exists(),
                "{pid} is alive"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_missing_loadgen_shows_failed_rows_and_no_restart() {
        let page = start_page(dead_config(), no_program()).await;
        let (status, answer) =
            http(page.addr, "POST", "/api/load/start?users=2&duration_s=5").await;
        assert_eq!(status, 500);
        assert_eq!(answer["ok"], false);
        assert_eq!(answer["failed"], 2);
        let (_, load) = http(page.addr, "GET", "/api/load").await;
        assert_eq!(load["running_programs"], 0);
        let rows = load["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["state"], "failed");
        assert!(
            rows[0]["summary"]
                .as_str()
                .unwrap()
                .contains("could not start")
        );
        page.end().await;
    }
}
