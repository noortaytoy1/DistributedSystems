//! Simulated users. Each user is a tokio task that reports incidents through one shared client.
//!
//! ```text
//! cargo run --release --bin loadgen -- --client-id 1 --users 20 --duration-s 30
//! cargo run --release --bin loadgen -- --client-id 1 --rate 500 --duration-s 30
//! ```
//!
//! Closed loop (default): each user sends, waits for the reply or gives up, thinks, repeats. Open
//! loop (`--rate`): requests go out at evenly spaced intended times whatever the replies do, and
//! latency counts from the intended time, so overload cannot hide in the generator (D54). Reports
//! then take turns among `--users` owners (D146).
//!
//! Every address comes from `--config`, so the same command works on all three client machines.
//! Prints throughput and percentiles; raw events go to `metrics.dir` from the config.
//!
//! Owner: F (client and scale).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rescuemesh::campus;
use rescuemesh::client::{ClientError, MeshClient, Submitted};
use rescuemesh::config::{CampusConfig, Config};
use rescuemesh::metrics::{EventLog, now_us};
use rescuemesh::proto::{ClientId, UserId};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::Instant;

/// Requests allowed in flight in open-loop mode; more wait, and their latency keeps counting.
const OPEN_LOOP_MAX_IN_FLIGHT: usize = 20_000;

#[derive(Parser)]
#[command(about = "Generate incident load against a RescueMesh cluster")]
struct Args {
    #[arg(long, default_value = "config/local.toml")]
    config: PathBuf,
    /// Unique per load generator, across all machines (D113).
    #[arg(long)]
    client_id: u64,
    /// Concurrent simulated users (closed loop). With `--rate`, how many users the reports are
    /// shared among; it does not change the rate.
    #[arg(long, default_value_t = 10)]
    users: u32,
    /// Sending time, including any warm-up.
    #[arg(long, default_value_t = 10)]
    duration_s: u64,
    /// Pause between a reply and the user's next request (closed loop).
    #[arg(long, default_value_t = 0)]
    think_ms: u64,
    /// Open loop: this many requests per second, whatever the replies do.
    #[arg(long)]
    rate: Option<u64>,
    /// Leave requests started in the first seconds out of the printed figures. Raw events are
    /// still logged.
    #[arg(long, default_value_t = 0)]
    warmup_s: u64,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// How long each report stays valid, instead of `campus.incident_ttl_ms`. For showing a refusal
    /// (0 refuses every report at once); the experiment runner never passes it.
    #[arg(long)]
    ttl_ms: Option<u64>,
}

#[derive(Default)]
struct Stats {
    latencies_us: Vec<u64>,
    gave_up: u64,
    no_responder: u64,
    retried: u64,
    provisional: u64,
}

impl Stats {
    fn record(&mut self, outcome: Result<Submitted, ClientError>) {
        match outcome {
            Ok(done) => {
                self.latencies_us.push(done.latency.as_micros() as u64);
                self.no_responder += u64::from(done.plan.responder.is_none());
                self.provisional += u64::from(done.provisional);
                self.retried += u64::from(done.attempts > 1);
            }
            Err(ClientError::GaveUp { .. }) => self.gave_up += 1,
            Err(e) => tracing::warn!("request failed: {e}"),
        }
    }
}

/// The value at `percent` of a sorted slice (nearest rank).
fn percentile(sorted: &[u64], percent: u64) -> Option<u64> {
    let rank = (sorted.len() as u64 * percent).div_ceil(100).max(1);
    sorted.get(rank as usize - 1).copied()
}

/// One draw from this run's seeded generator, stamped with the current time.
fn incident(rng: &mut StdRng, campus: &CampusConfig, owner: UserId) -> rescuemesh::proto::Incident {
    campus::random_incident(rng, campus, owner, now_us() / 1000)
}

/// Each user draws from its own generator, so a seed repeats the same incidents per user.
fn user_rng(seed: u64, user: u64) -> StdRng {
    StdRng::seed_from_u64(seed.wrapping_mul(1_000_003).wrapping_add(user))
}

/// Who reports the next open-loop request: the users take turns. `.max(1)` keeps `--users 0`, which
/// open loop used to ignore, from dividing by zero.
fn open_loop_owner(client: ClientId, sent: u32, users: u32) -> UserId {
    UserId {
        client,
        user: sent % users.max(1),
    }
}

async fn closed_loop(
    args: &Args,
    client: Arc<MeshClient>,
    campus: CampusConfig,
    measure_from: Instant,
    stop_at: Instant,
    stats: Arc<Mutex<Stats>>,
) {
    let think = Duration::from_millis(args.think_ms);
    let mut users = JoinSet::new();
    for user in 0..args.users {
        let (client, stats) = (client.clone(), stats.clone());
        let mut rng = user_rng(args.seed, u64::from(user));
        let owner = UserId {
            client: args.client_id,
            user,
        };
        users.spawn(async move {
            while Instant::now() < stop_at {
                let started = Instant::now();
                let outcome = client.submit(incident(&mut rng, &campus, owner)).await;
                if started >= measure_from {
                    lock(&stats).record(outcome);
                }
                if !think.is_zero() {
                    tokio::time::sleep(think).await;
                }
            }
        });
    }
    while users.join_next().await.is_some() {}
}

async fn open_loop(
    args: &Args,
    rate: u64,
    client: Arc<MeshClient>,
    campus: CampusConfig,
    measure_from: Instant,
    stop_at: Instant,
    stats: Arc<Mutex<Stats>>,
) {
    let start = Instant::now();
    let gap = Duration::from_secs(1) / u32::try_from(rate.max(1)).unwrap_or(u32::MAX);
    let slots = Arc::new(Semaphore::new(OPEN_LOOP_MAX_IN_FLIGHT));
    let mut rng = user_rng(args.seed, 0);
    let mut requests = JoinSet::new();
    let mut sent: u32 = 0;
    loop {
        let intended = start + gap * sent;
        if intended >= stop_at {
            break;
        }
        tokio::time::sleep_until(intended).await;
        while requests.try_join_next().is_some() {}
        let Ok(slot) = slots.clone().acquire_owned().await else {
            break;
        };
        let (client, stats) = (client.clone(), stats.clone());
        let owner = open_loop_owner(args.client_id, sent, args.users);
        let report = incident(&mut rng, &campus, owner);
        requests.spawn(async move {
            let outcome = client.submit_from(report, intended).await;
            if intended >= measure_from {
                lock(&stats).record(outcome);
            }
            drop(slot);
        });
        sent += 1;
    }
    while requests.join_next().await.is_some() {}
}

fn print_summary(args: &Args, stats: &mut Stats, measured: Duration) {
    stats.latencies_us.sort_unstable();
    let done = stats.latencies_us.len() as u64;
    let shown = |percent| {
        percentile(&stats.latencies_us, percent).map_or_else(
            || "-".to_string(),
            |us| format!("{:.2}", us as f64 / 1000.0),
        )
    };
    let mean_ms = stats.latencies_us.iter().sum::<u64>() as f64 / done.max(1) as f64 / 1000.0;
    println!(
        "client {}: done {done}, gave up {}, retried {}, no responder {}, provisional {}",
        args.client_id, stats.gave_up, stats.retried, stats.no_responder, stats.provisional
    );
    println!(
        "throughput {:.1} req/s over {:.1} s",
        done as f64 / measured.as_secs_f64().max(0.001),
        measured.as_secs_f64()
    );
    println!(
        "latency ms: mean {mean_ms:.2}, p50 {}, p95 {}, p99 {}",
        shown(50),
        shown(95),
        shown(99)
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rescuemesh::init_tracing();
    let args = Args::parse();
    let config =
        Config::load(&args.config).with_context(|| format!("loading {}", args.config.display()))?;
    anyhow::ensure!(
        args.warmup_s < args.duration_s,
        "--warmup-s must be shorter than --duration-s"
    );
    let log = EventLog::open(
        config.metrics.dir.as_deref(),
        &format!("client-{}", args.client_id),
    )
    .context("opening the event log")?;
    let client = Arc::new(MeshClient::new(args.client_id, &config, log.clone()));
    let stats = Arc::new(Mutex::new(Stats::default()));

    let start = Instant::now();
    let measure_from = start + Duration::from_secs(args.warmup_s);
    let stop_at = start + Duration::from_secs(args.duration_s);
    let shared = stats.clone();
    let mut campus = config.campus;
    if let Some(ttl_ms) = args.ttl_ms {
        campus.incident_ttl_ms = ttl_ms;
    }
    match args.rate {
        Some(rate) => {
            open_loop(
                &args,
                rate,
                client.clone(),
                campus,
                measure_from,
                stop_at,
                shared,
            )
            .await;
        }
        None => {
            closed_loop(&args, client.clone(), campus, measure_from, stop_at, shared).await;
        }
    }

    let measured = Duration::from_secs(args.duration_s - args.warmup_s);
    print_summary(&args, &mut lock(&stats), measured);
    drop(client);
    tokio::task::spawn_blocking(move || log.close())
        .await
        .context("closing the event log")?;
    Ok(())
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let values: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&values, 50), Some(50));
        assert_eq!(percentile(&values, 95), Some(95));
        assert_eq!(percentile(&values, 99), Some(99));
        assert_eq!(percentile(&[7], 99), Some(7));
        assert_eq!(percentile(&[], 50), None);
    }

    #[test]
    fn open_loop_owner_survives_zero_users() {
        assert_eq!(open_loop_owner(4, 7, 0), UserId { client: 4, user: 0 });
    }

    #[test]
    fn open_loop_reports_take_turns_among_the_users() {
        let users: Vec<u32> = (0..5).map(|s| open_loop_owner(4, s, 3).user).collect();
        assert_eq!(users, vec![0, 1, 2, 0, 1]);
    }

    #[test]
    fn a_seed_repeats_the_same_incidents_and_users_differ() {
        let campus = CampusConfig::default();
        let owner = UserId { client: 1, user: 0 };
        let draw = |user| campus::random_incident(&mut user_rng(5, user), &campus, owner, 0);
        assert_eq!(draw(1), draw(1));
        assert_ne!(
            (1..20).map(draw).collect::<Vec<_>>(),
            (2..21).map(draw).collect::<Vec<_>>()
        );
    }
}
