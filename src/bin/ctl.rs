//! Operator tool: query nodes, read incident records, silence a node, skew load.
//!
//! ```text
//! cargo run --bin ctl -- status
//! cargo run --bin ctl -- incidents --client 3 --limit 5
//! cargo run --bin ctl -- incidents --id 3-1762180000000123 --node 2
//! cargo run --bin ctl -- discover --zone 3 --capability medical
//! cargo run --bin ctl -- silence --node 2 --ms 5000
//! cargo run --bin ctl -- skew --node 1 --work 200000
//! ```
//!
//! Owner: C (faults and failover).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use rescuemesh::client::{control, udp_answers};
use rescuemesh::config::{Config, NodeAddr};
use rescuemesh::node::transport::connect_client;
use rescuemesh::proto::{
    Capability, ControlReply, ControlRequest, DEFAULT_INCIDENT_ROWS, DiscoverReply, IncidentQuery,
    IncidentView, MAX_DISCOVER_RECORDS, NodeStatus, PresenceQuery, PresenceRecord, RequestId,
};

#[derive(Parser)]
#[command(about = "Inspect and inject faults into a RescueMesh cluster")]
struct Args {
    #[arg(long, default_value = "config/local.toml")]
    config: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show term, leader, log position, state digest and group members of every node, and whether
    /// its UDP port, which reports travel on, answers.
    Status,
    /// Read incident records from every node, or from the one `--node` names.
    Incidents {
        /// The newest records of this client.
        #[arg(long, required_unless_present = "id", conflicts_with = "id")]
        client: Option<u64>,
        /// How many of them. A node returns at most 20.
        #[arg(long, default_value_t = DEFAULT_INCIDENT_ROWS)]
        limit: u32,
        /// One record, by the ID in `<client>-<seq>` form.
        #[arg(long)]
        id: Option<RequestId>,
        #[arg(long)]
        node: Option<u64>,
    },
    /// Ask every node's presence directory who is online, side by side, for demo scenario 5 (D295).
    /// Each node answers from its own copy, labelled with the log index it read at.
    Discover {
        #[arg(long)]
        zone: Option<u16>,
        /// medical, fire, technical or rescue.
        #[arg(long, value_parser = parse_capability)]
        capability: Option<Capability>,
        #[arg(long, default_value_t = MAX_DISCOVER_RECORDS)]
        limit: u16,
    },
    /// Drop all traffic in and out of a node for a while.
    Silence {
        #[arg(long)]
        node: u64,
        #[arg(long, default_value_t = 5000)]
        ms: u64,
    },
    /// Set the planner work per request on one node, to create load imbalance. It replaces the
    /// node's plan_work, so to undo it pass the value from the config, not 0.
    Skew {
        #[arg(long)]
        node: u64,
        #[arg(long)]
        work: u32,
    },
}

const TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rescuemesh::init_tracing();
    let args = Args::parse();
    let config =
        Config::load(&args.config).with_context(|| format!("loading {}", args.config.display()))?;

    match args.cmd {
        Cmd::Status => status(&config).await,
        Cmd::Incidents {
            client,
            limit,
            id,
            node,
        } => {
            let query = match (id, client) {
                (Some(id), _) => IncidentQuery::One(id),
                (None, Some(client)) => IncidentQuery::OfClient { client, limit },
                (None, None) => bail!("give --client or --id"),
            };
            incidents(&config, node, query).await
        }
        Cmd::Discover {
            zone,
            capability,
            limit,
        } => {
            discover(
                &config,
                PresenceQuery {
                    zone,
                    capability,
                    limit,
                },
            )
            .await
        }
        Cmd::Silence { node, ms } => send(&config, node, ControlRequest::Silence { ms }).await,
        Cmd::Skew { node, work } => {
            send(&config, node, ControlRequest::SkewLoad { extra_work: work }).await
        }
    }
}

async fn status(config: &Config) -> anyhow::Result<()> {
    println!(
        "{:>4} {:>10} {:>6} {:>6} {:>8} {:>8} {:>18} {:>8} {:>7} {:>8} {:>8} {:>7}",
        "node",
        "role",
        "term",
        "leader",
        "log",
        "applied",
        "digest",
        "results",
        "serving",
        "silenced",
        "degraded",
        "members"
    );
    for n in &config.cluster.nodes {
        match control(&n.client_addr, ControlRequest::Status, TIMEOUT).await {
            Ok(ControlReply::Status(s)) => println!(
                "{:>4} {:>10} {:>6} {:>6} {:>8} {:>8} {:>18} {:>8} {:>7} {:>8} {:>8} {:>7}",
                s.id,
                s.role,
                s.term,
                opt(s.leader),
                opt(s.last_log_index),
                opt(s.last_applied),
                format!("{:016x}", s.digest),
                s.results,
                s.serving,
                s.silenced,
                s.degraded,
                members_column(&s)
            ),
            Ok(other) => println!("{:>4} unexpected reply: {other:?}", n.id),
            Err(e) => println!("{:>4} unreachable: {e}", n.id),
        }
    }
    check_udp(config).await;
    Ok(())
}

/// Below the table, so the columns stay where scripts read them. This table comes over TCP, so a
/// missing UDP firewall rule shows nowhere in it: the node looks healthy while every report to it
/// vanishes (D168).
async fn check_udp(config: &Config) {
    let checks = config.cluster.nodes.iter().map(|n| async move {
        let answered = udp_answers(&n.client_addr, TIMEOUT).await;
        udp_line(n, answered)
    });
    for line in futures::future::join_all(checks).await {
        println!("{line}");
    }
}

fn udp_line(node: &NodeAddr, answered: bool) -> String {
    if answered {
        return format!("node {}: udp ok", node.id);
    }
    let port = node.client_addr.rsplit(':').next().unwrap_or("?");
    format!(
        "node {}: udp no answer. Reports to it vanish: is it running, and is the port open? \
         sudo ufw allow {port}/udp",
        node.id
    )
}

/// Asks the nodes one after another, so under load their lists can shift between two answers. To
/// compare nodes, compare one ID: a brief Assigned against Resolved is catch-up, and a `log_index`
/// or digest that stays different is a bug.
async fn incidents(config: &Config, only: Option<u64>, query: IncidentQuery) -> anyhow::Result<()> {
    if let Some(node) = only.filter(|node| config.node(*node).is_none()) {
        bail!("node {node} is not in the config");
    }
    let nodes = config.cluster.nodes.iter();
    for n in nodes.filter(|n| only.is_none_or(|node| node == n.id)) {
        let request = ControlRequest::Incidents(query.clone());
        match control(&n.client_addr, request, TIMEOUT).await {
            Ok(ControlReply::Incidents { node, rows }) => {
                println!("{}", node_line(&node));
                if !node.serving || node.silenced {
                    println!("  (silenced or catching up: these rows may be behind)");
                }
                if rows.is_empty() {
                    println!("  (no records)");
                }
                for row in &rows {
                    println!("  {}", row_line(row));
                }
            }
            Ok(other) => println!("node {}: unexpected reply: {other:?}", n.id),
            Err(e) => println!("node {}: unreachable: {e}", n.id),
        }
    }
    Ok(())
}

fn parse_capability(text: &str) -> Result<Capability, String> {
    Capability::ALL
        .into_iter()
        .find(|c| format!("{c:?}").eq_ignore_ascii_case(text))
        .ok_or_else(|| format!("unknown capability {text:?}: medical, fire, technical or rescue"))
}

/// Asks each node in turn over TCP. A node that is not serving, or is silenced, stays quiet, which
/// shows as no answer. The last line says whether the nodes that answered at the same index agree.
async fn discover(config: &Config, query: PresenceQuery) -> anyhow::Result<()> {
    let mut answers: Vec<(u64, u64, Vec<PresenceRecord>)> = Vec::new();
    for n in &config.cluster.nodes {
        let call = async {
            let tarpc::client::NewClient { client, dispatch } =
                connect_client(&n.client_addr).await?;
            let dispatch = tokio::spawn(dispatch);
            let mut context = tarpc::context::current();
            context.deadline = std::time::Instant::now() + TIMEOUT;
            let reply = client.discover(context, query.clone()).await;
            dispatch.abort();
            anyhow::Ok(reply?)
        };
        match tokio::time::timeout(TIMEOUT, call).await {
            Ok(Ok(DiscoverReply::Found { records, log_index })) => {
                println!(
                    "node {}: log_index={log_index} records={}",
                    n.id,
                    records.len()
                );
                for r in &records {
                    println!("  {}", record_line(r));
                }
                answers.push((n.id, log_index, records));
            }
            Ok(Ok(DiscoverReply::Rejected(reason))) => println!("node {}: refused: {reason}", n.id),
            Ok(Err(e)) => println!("node {}: unreachable: {e}", n.id),
            Err(_) => println!("node {}: no answer (silenced or catching up)", n.id),
        }
    }
    println!("{}", agreement(&answers));
    Ok(())
}

/// Whether the answers agree. Answers read at different indexes may differ for a moment: that is
/// catch-up, not disagreement, so it is said apart (D288).
fn agreement(answers: &[(u64, u64, Vec<PresenceRecord>)]) -> String {
    let Some((_, index, records)) = answers.first() else {
        return "no node answered".into();
    };
    if answers.iter().all(|(_, _, r)| r == records) {
        return format!("same answer from {} nodes", answers.len());
    }
    if answers.iter().all(|(_, i, _)| i == index) {
        return "DIFFERENT answers at the same log index: a bug".into();
    }
    "answers differ, read at different log indexes: ask again".into()
}

fn record_line(r: &PresenceRecord) -> String {
    let capabilities: Vec<String> = r.capabilities.iter().map(|c| format!("{c:?}")).collect();
    format!(
        "{} role={:?} zone={} peer_addr={} capabilities={} incarnation={}",
        r.user,
        r.role,
        r.zone,
        r.peer_addr,
        capabilities.join(","),
        r.incarnation
    )
}

fn node_line(s: &NodeStatus) -> String {
    format!(
        "node {}: role={} applied={} digest={:016x} serving={} silenced={}",
        s.id,
        s.role,
        opt(s.last_applied),
        s.digest,
        s.serving,
        s.silenced
    )
}

/// Every field of a record on one line, `key=value`, so a grep can pick out what it needs.
fn row_line(row: &IncidentView) -> String {
    let incident = &row.incident;
    format!(
        "{} status={} responder={} priority={} owner={} category={:?} needs={:?} zone={} \
         location={},{} severity={} reported_at_ms={} ttl_ms={} age_ms={} assigned_at_ms={} \
         expires_at_ms={} executor={} log_index={}",
        row.id,
        row.status,
        opt(row.responder),
        row.priority,
        incident.owner,
        incident.category,
        incident.needs,
        incident.zone,
        incident.location.x,
        incident.location.y,
        incident.severity,
        incident.reported_at_ms,
        incident.ttl_ms,
        row.age_ms,
        row.assigned_at_ms,
        opt(row.expires_at_ms),
        row.executor,
        row.log_index
    )
}

async fn send(config: &Config, node: u64, request: ControlRequest) -> anyhow::Result<()> {
    let Some(addr) = config.node(node).map(|n| n.client_addr.clone()) else {
        bail!("node {node} is not in the config");
    };
    match control(&addr, request, TIMEOUT).await? {
        ControlReply::Done => {
            println!("ok");
            Ok(())
        }
        ControlReply::Error(e) => bail!("node {node}: {e}"),
        other => bail!("node {node}: unexpected reply {other:?}"),
    }
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
}

/// `1,3`, or `-` while silenced: the list is frozen then, and must not read as a live one (D155).
fn members_column(status: &NodeStatus) -> String {
    if status.silenced {
        return "-".into();
    }
    let members: Vec<String> = status.members.iter().map(ToString::to_string).collect();
    members.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rescuemesh::proto::{Capability, Category, Incident, IncidentStatus, Point, UserId};

    fn status(silenced: bool, members: Vec<u64>) -> NodeStatus {
        NodeStatus {
            id: 1,
            term: 1,
            leader: Some(1),
            role: "leader".into(),
            last_log_index: None,
            last_applied: None,
            digest: 0,
            results: 0,
            responders: 0,
            silenced,
            serving: !silenced,
            degraded: false,
            members,
        }
    }

    #[test]
    fn members_print_as_a_list_and_as_a_dash_while_silenced() {
        assert_eq!(members_column(&status(false, vec![1, 3])), "1,3");
        assert_eq!(members_column(&status(false, vec![3])), "3");
        assert_eq!(members_column(&status(true, vec![1, 2, 3])), "-");
    }

    #[test]
    fn the_udp_line_names_the_node_and_the_rule_to_add() {
        let node = NodeAddr {
            id: 2,
            peer_addr: "10.0.0.12:7001".into(),
            client_addr: "10.0.0.12:8001".into(),
        };
        assert_eq!(udp_line(&node, true), "node 2: udp ok");
        let line = udp_line(&node, false);
        assert!(line.starts_with("node 2: udp no answer"), "{line}");
        assert!(line.contains("ufw allow 8001/udp"), "{line}");
    }

    fn parse(line: &[&str]) -> Result<Args, clap::Error> {
        Args::try_parse_from(std::iter::once("ctl").chain(line.iter().copied()))
    }

    #[test]
    fn incidents_take_a_client_or_an_id_but_not_both() {
        let Cmd::Incidents {
            client,
            limit,
            id,
            node,
        } = parse(&["incidents", "--client", "3"]).unwrap().cmd
        else {
            panic!("wrong command");
        };
        assert_eq!((client, limit, id, node), (Some(3), 10, None, None));

        let Cmd::Incidents { id, node, .. } = parse(&["incidents", "--id", "3-17", "--node", "2"])
            .unwrap()
            .cmd
        else {
            panic!("wrong command");
        };
        assert_eq!(id, Some(RequestId { client: 3, seq: 17 }));
        assert_eq!(node, Some(2));

        assert!(parse(&["incidents"]).is_err());
        assert!(parse(&["incidents", "--client", "3", "--id", "3-17"]).is_err());
        assert!(parse(&["incidents", "--id", "not-an-id"]).is_err());
    }

    #[test]
    fn a_row_prints_every_field() {
        let row = IncidentView {
            id: RequestId { client: 3, seq: 17 },
            incident: Incident {
                owner: UserId {
                    client: 3,
                    user: 12,
                },
                category: Category::Flooding,
                zone: 4,
                location: Point { x: 210, y: 720 },
                severity: 5,
                needs: Capability::Rescue,
                reported_at_ms: 1000,
                ttl_ms: 60000,
            },
            status: IncidentStatus::Assigned,
            responder: Some(16),
            priority: 460,
            executor: 2,
            age_ms: 8,
            assigned_at_ms: 1022,
            expires_at_ms: Some(2022),
            log_index: 57,
        };
        assert_eq!(
            row_line(&row),
            "3-17 status=assigned responder=16 priority=460 owner=3/12 category=Flooding \
             needs=Rescue zone=4 location=210,720 severity=5 reported_at_ms=1000 ttl_ms=60000 \
             age_ms=8 assigned_at_ms=1022 expires_at_ms=2022 executor=2 log_index=57"
        );
        let nobody = IncidentView {
            responder: None,
            expires_at_ms: None,
            ..row
        };
        assert!(row_line(&nobody).contains("responder=- "));
        assert!(row_line(&nobody).contains("expires_at_ms=- "));
    }

    #[test]
    fn capabilities_parse_in_any_case_and_answers_are_compared() {
        assert_eq!(parse_capability("Medical"), Ok(Capability::Medical));
        assert_eq!(parse_capability("rescue"), Ok(Capability::Rescue));
        assert!(parse_capability("plumbing").is_err());
        assert_eq!(agreement(&[]), "no node answered");
        let same = vec![(1, 5, vec![]), (2, 5, vec![])];
        assert!(agreement(&same).starts_with("same"));
        let record = PresenceRecord {
            user: UserId { client: 1, user: 2 },
            role: rescuemesh::proto::PresenceRole::Ordinary,
            zone: 0,
            peer_addr: "a:1".into(),
            capabilities: vec![],
            incarnation: 1,
        };
        assert!(agreement(&[(1, 5, vec![record.clone()]), (2, 5, vec![])]).contains("bug"));
        assert!(agreement(&[(1, 5, vec![record]), (2, 6, vec![])]).contains("ask again"));
    }
}
