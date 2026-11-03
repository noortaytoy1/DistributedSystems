//! Operator tool: query nodes, read incident records, silence a node, skew load.
//!
//! ```text
//! cargo run --bin ctl -- status
//! cargo run --bin ctl -- incidents --client 3 --limit 5
//! cargo run --bin ctl -- incidents --id 3-1762180000000123 --node 2
//! cargo run --bin ctl -- silence --node 2 --ms 5000
//! cargo run --bin ctl -- skew --node 1 --work 200000
//! ```
//!
//! Owner: C (faults and failover).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use rescuemesh::client::control;
use rescuemesh::config::Config;
use rescuemesh::proto::{
    ControlReply, ControlRequest, DEFAULT_INCIDENT_ROWS, IncidentQuery, IncidentView, NodeStatus,
    RequestId,
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
    /// Show term, leader, log position and state digest of every node.
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
    /// Drop all traffic in and out of a node for a while.
    Silence {
        #[arg(long)]
        node: u64,
        #[arg(long, default_value_t = 5000)]
        ms: u64,
    },
    /// Add planner work per request on one node to create load imbalance.
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
        Cmd::Silence { node, ms } => send(&config, node, ControlRequest::Silence { ms }).await,
        Cmd::Skew { node, work } => {
            send(&config, node, ControlRequest::SkewLoad { extra_work: work }).await
        }
    }
}

async fn status(config: &Config) -> anyhow::Result<()> {
    println!(
        "{:>4} {:>10} {:>6} {:>6} {:>8} {:>8} {:>18} {:>8} {:>7} {:>8} {:>8}",
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
        "degraded"
    );
    for n in &config.cluster.nodes {
        match control(&n.client_addr, ControlRequest::Status, TIMEOUT).await {
            Ok(ControlReply::Status(s)) => println!(
                "{:>4} {:>10} {:>6} {:>6} {:>8} {:>8} {:>18} {:>8} {:>7} {:>8} {:>8}",
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
                s.degraded
            ),
            Ok(other) => println!("{:>4} unexpected reply: {other:?}", n.id),
            Err(e) => println!("{:>4} unreachable: {e}", n.id),
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use rescuemesh::proto::{Capability, Category, Incident, IncidentStatus, Point, UserId};

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
}
