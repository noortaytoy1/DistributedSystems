//! Simulated users. One program holds many users, each a tokio task that reports incidents through
//! one shared client. For real load use `scripts/start-users.sh`, which shares the users out over
//! several of these programs (D181).
//!
//! This is the application layer on the client side (D211). A user calls `MeshClient::submit`, which
//! comes from `rescuemesh::middleware`, and never names a node or sees the copies behind that call.
//!
//! ```text
//! cargo run --release --bin loadgen -- --client-id 1 --users 20 --duration-s 30
//! cargo run --release --bin loadgen -- --client-id 1 --users 20 --think-ms 1000 --pattern random
//! cargo run --release --bin loadgen -- --client-id 1 --rate 500 --duration-s 30
//! ```
//!
//! Closed loop (default): each user sends, waits for the reply or gives up, waits as `--pattern`
//! says, repeats (D182). Open loop (`--rate`): requests go out at evenly spaced intended times
//! whatever the replies do, and latency counts from the intended time, so overload cannot hide in
//! the generator (D54). Reports then take turns among `--users` owners (D146).
//!
//! Every address comes from `--config`, so the same command works on all three client machines.
//! Prints throughput and percentiles; raw events go to `metrics.dir` from the config. TERM or
//! Ctrl-C ends the run early and still prints the summary and flushes the log (D183).
//!
//! Owner: F (client and scale).

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, ValueEnum};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rescuemesh::campus;
use rescuemesh::config::{CampusConfig, Config};
use rescuemesh::metrics::{self, Event, EventLog, now_ms};
use rescuemesh::middleware::{ClientError, MeshClient, Submitted};
use rescuemesh::proto::{ClientId, IncidentStatus, UserId};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::Instant;

/// Requests allowed in flight in open-loop mode; more wait, and their latency keeps counting.
const OPEN_LOOP_MAX_IN_FLIGHT: usize = 20_000;
/// How often `--sample-machine` reads the machine's CPU and memory (D185).
const MACHINE_SAMPLE: Duration = Duration::from_secs(1);

/// How a user waits between requests, in a closed loop (D182).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Pattern {
    /// Exactly the think time after each reply, and first sends spread evenly over one think time.
    Steady,
    /// A random wait from 0 to twice the think time, so the mean load matches `steady`.
    Random,
    /// Every user sends on the same multiple of the think time on the wall clock: spikes.
    AllAtOnce,
}

#[derive(Parser)]
#[command(about = "Generate incident load against a RescueMesh cluster")]
struct Args {
    #[arg(long, default_value = "config/local.toml")]
    config: PathBuf,
    /// Unique per load generator, across all machines (D113). By hand use 1 to 9: 11 to 13 belong
    /// to frontends, 21 to 23 to zone feeds, and five-digit IDs to scripts/start-users.sh (D184).
    #[arg(long)]
    client_id: u64,
    /// Concurrent simulated users (closed loop). With `--rate`, how many users the reports are
    /// shared among; it does not change the rate.
    #[arg(long, default_value_t = 10)]
    users: u32,
    /// Sending time, including any warm-up.
    #[arg(long, default_value_t = 10)]
    duration_s: u64,
    /// Pause between a reply and the user's next request (closed loop). `--pattern` says how it
    /// is used.
    #[arg(long, default_value_t = 0)]
    think_ms: u64,
    /// How users wait between requests (closed loop).
    #[arg(long, value_enum, default_value_t = Pattern::Steady, conflicts_with = "rate")]
    pattern: Pattern,
    /// Open loop: this many requests per second, whatever the replies do.
    #[arg(long)]
    rate: Option<u64>,
    /// Leave requests started in the first seconds out of the printed figures. Raw events are
    /// still logged.
    #[arg(long, default_value_t = 0)]
    warmup_s: u64,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Also log the whole machine's CPU and memory once a second. One program per client PC is
    /// enough (D185).
    #[arg(long)]
    sample_machine: bool,
    /// How long each report stays valid, instead of `campus.incident_ttl_ms`. For showing a refusal
    /// (0 refuses every report at once); the experiment runner never passes it.
    #[arg(long)]
    ttl_ms: Option<u64>,
}

impl Args {
    fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.warmup_s < self.duration_s,
            "--warmup-s must be shorter than --duration-s"
        );
        // With no think time these patterns have nothing to randomise or to line up on.
        anyhow::ensure!(
            self.pattern == Pattern::Steady || self.think_ms > 0,
            "--pattern {:?} needs --think-ms above 0",
            self.pattern
        );
        Ok(())
    }
}

#[derive(Default)]
struct Stats {
    latencies_us: Vec<u64>,
    gave_up: u64,
    no_responder: u64,
    retried: u64,
    provisional: u64,
    expired: u64,
}

impl Stats {
    fn record(&mut self, outcome: Result<Submitted, ClientError>) {
        match outcome {
            Ok(done) => {
                self.latencies_us.push(done.latency.as_micros() as u64);
                self.no_responder += u64::from(done.plan.responder.is_none());
                self.provisional += u64::from(done.provisional);
                self.retried += u64::from(done.attempts > 1);
                self.expired += u64::from(done.status == IncidentStatus::Expired);
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
    campus::random_incident(rng, campus, owner, now_ms())
}

/// Each user draws from its own generator, so a seed repeats the same incidents per user.
fn user_rng(seed: u64, user: u64) -> StdRng {
    StdRng::seed_from_u64(seed.wrapping_mul(1_000_003).wrapping_add(user))
}

/// One user's waits. It has its own generator, so `random` never disturbs the incident stream a
/// seed repeats.
struct Pacer {
    pattern: Pattern,
    think_ms: u64,
    rng: StdRng,
}

impl Pacer {
    fn new(pattern: Pattern, think_ms: u64, seed: u64, user: u64) -> Self {
        let stream = seed.wrapping_mul(1_000_003).wrapping_add(user) ^ 0x5741_4954;
        Self {
            pattern,
            think_ms,
            rng: StdRng::seed_from_u64(stream),
        }
    }

    /// The wait before this user's first request. Under `steady`, user `j` of `users` gets its own
    /// slot in the first think time: a whole program sending at once is what started the term
    /// storm in the rehearsal (D182).
    fn first_wait(&mut self, user: u32, users: u32, now_ms: u64) -> Duration {
        match self.pattern {
            Pattern::Steady => {
                Duration::from_millis(self.think_ms).saturating_mul(user) / users.max(1)
            }
            Pattern::Random | Pattern::AllAtOnce => self.next_wait(now_ms),
        }
    }

    /// The wait between a reply and this user's next request.
    fn next_wait(&mut self, now_ms: u64) -> Duration {
        Duration::from_millis(match self.pattern {
            Pattern::Steady => self.think_ms,
            Pattern::Random => self.rng.gen_range(0..=self.think_ms.saturating_mul(2)),
            // The wall clock is what lines users up across programs and machines. `Args::check`
            // keeps think_ms above 0, and the `max` keeps a mistake from dividing by zero.
            Pattern::AllAtOnce => self.think_ms - now_ms % self.think_ms.max(1),
        })
    }
}

/// Wait, but never past the end of the run, so a long wait cannot make a program outlive it.
async fn pause(wait: Duration, stop_at: Instant) {
    if !wait.is_zero() {
        let until = Instant::now().checked_add(wait).unwrap_or(stop_at);
        tokio::time::sleep_until(until.min(stop_at)).await;
    }
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
    let total = args.users;
    let mut users = JoinSet::new();
    for user in 0..total {
        let (client, stats) = (client.clone(), stats.clone());
        let mut rng = user_rng(args.seed, u64::from(user));
        let mut pacer = Pacer::new(args.pattern, args.think_ms, args.seed, u64::from(user));
        let owner = UserId {
            client: args.client_id,
            user,
        };
        users.spawn(async move {
            pause(pacer.first_wait(user, total, now_ms()), stop_at).await;
            while Instant::now() < stop_at {
                let started = Instant::now();
                let outcome = client.submit(incident(&mut rng, &campus, owner)).await;
                if started >= measure_from {
                    lock(&stats).record(outcome);
                }
                pause(pacer.next_wait(now_ms()), stop_at).await;
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

/// Once a second until aborted. The reads are blocking file reads, so they go off the runtime.
async fn sample_machine(log: EventLog) {
    let mut ticks = tokio::time::interval(MACHINE_SAMPLE);
    loop {
        ticks.tick().await;
        match tokio::task::spawn_blocking(metrics::machine_usage).await {
            Ok(usage) => log.log(usage),
            Err(e) => tracing::warn!("cannot read machine usage: {e}"),
        }
    }
}

/// One string, for one write: many programs share one output file, and a block must not mix with
/// another program's. `scripts/start-users.sh` adds up the first line, so its wording is a contract.
fn summary(client_id: u64, stats: &mut Stats, measured: Duration, stopped_early: bool) -> String {
    stats.latencies_us.sort_unstable();
    let done = stats.latencies_us.len() as u64;
    let shown = |percent| {
        percentile(&stats.latencies_us, percent).map_or_else(
            || "-".to_string(),
            |us| format!("{:.2}", us as f64 / 1000.0),
        )
    };
    let mean_ms = stats.latencies_us.iter().sum::<u64>() as f64 / done.max(1) as f64 / 1000.0;
    let note = if stopped_early {
        "stopped by a signal: requests still in flight are not counted\n"
    } else {
        ""
    };
    format!(
        "client {client_id}: done {done}, gave up {}, retried {}, no responder {}, provisional {}, \
         expired {}\n\
         {note}throughput {:.1} req/s over {:.1} s\n\
         latency ms: mean {mean_ms:.2}, p50 {}, p95 {}, p99 {}\n",
        stats.gave_up,
        stats.retried,
        stats.no_responder,
        stats.provisional,
        stats.expired,
        done as f64 / measured.as_secs_f64().max(0.001),
        measured.as_secs_f64(),
        shown(50),
        shown(95),
        shown(99)
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rescuemesh::init_tracing();
    let args = Args::parse();
    let config =
        Config::load(&args.config).with_context(|| format!("loading {}", args.config.display()))?;
    args.check()?;
    let log = EventLog::open(
        config.metrics.dir.as_deref(),
        &format!("client-{}", args.client_id),
    )
    .context("opening the event log")?;
    let client = Arc::new(MeshClient::new(args.client_id, &config, log.clone()));
    let stats = Arc::new(Mutex::new(Stats::default()));
    let sampler = args
        .sample_machine
        .then(|| tokio::spawn(sample_machine(log.clone())));

    let start = Instant::now();
    log.log(Event::ClientStarted {
        users: args.rate.map_or(args.users, |_| 0),
    });
    let measure_from = start + Duration::from_secs(args.warmup_s);
    let stop_at = start + Duration::from_secs(args.duration_s);
    let shared = stats.clone();
    let mut campus = config.campus;
    if let Some(ttl_ms) = args.ttl_ms {
        campus.incident_ttl_ms = ttl_ms;
    }
    let run = async {
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
    };
    // Losing the race drops the run and with it every user task; what they recorded stays.
    let stopped_early = tokio::select! {
        () = run => false,
        () = rescuemesh::shutdown_signal() => true,
    };
    if let Some(sampler) = sampler {
        sampler.abort();
    }

    let measured = if stopped_early {
        Instant::now().saturating_duration_since(measure_from)
    } else {
        Duration::from_secs(args.duration_s - args.warmup_s)
    };
    let text = summary(args.client_id, &mut lock(&stats), measured, stopped_early);
    drop(client);
    let closed = tokio::task::spawn_blocking(move || log.close()).await;
    // After the log, and no panic if it fails: `print!` panics when the reader has gone or the disk is
    // full, which must not cost the log or count as a failed run. The script says who reported.
    let mut out = std::io::stdout();
    let _ = out.write_all(text.as_bytes()).and_then(|()| out.flush());
    closed.context("closing the event log")
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

    fn pacer(pattern: Pattern, think_ms: u64) -> Pacer {
        Pacer::new(pattern, think_ms, 5, 1)
    }

    #[test]
    fn steady_first_waits_are_even_and_zero_for_one_user() {
        let mut p = pacer(Pattern::Steady, 1000);
        let firsts: Vec<_> = (0..4).map(|user| p.first_wait(user, 4, 123)).collect();
        assert_eq!(
            firsts,
            [0, 250, 500, 750].map(Duration::from_millis).to_vec()
        );
        assert_eq!(p.first_wait(0, 1, 123), Duration::ZERO);
        assert_eq!(
            pacer(Pattern::Steady, 0).first_wait(3, 4, 123),
            Duration::ZERO
        );
    }

    #[test]
    fn steady_waits_exactly_the_think_time() {
        let mut p = pacer(Pattern::Steady, 1000);
        assert_eq!(p.next_wait(123), Duration::from_millis(1000));
        assert_eq!(p.next_wait(999_999), Duration::from_millis(1000));
    }

    #[test]
    fn random_waits_stay_within_twice_the_think_time_and_repeat_for_a_seed() {
        let waits = |seed| {
            let mut p = Pacer::new(Pattern::Random, 1000, seed, 1);
            (0..2000).map(|_| p.next_wait(0)).collect::<Vec<_>>()
        };
        let first = waits(5);
        assert_eq!(first, waits(5));
        assert_ne!(first, waits(6));
        assert!(first.iter().all(|w| *w <= Duration::from_millis(2000)));
        // The whole range gets used, and the mean stays near the think time: the same load as
        // steady. These bounds are far wider than 2000 draws can stray.
        assert!(first.iter().any(|w| *w < Duration::from_millis(100)));
        assert!(first.iter().any(|w| *w > Duration::from_millis(1900)));
        let mean = first.iter().sum::<Duration>() / 2000;
        assert!(mean > Duration::from_millis(900) && mean < Duration::from_millis(1100));
    }

    #[test]
    fn random_waits_leave_the_incident_stream_unchanged() {
        let campus = CampusConfig::default();
        let owner = UserId { client: 1, user: 0 };
        let draw = |rng: &mut StdRng| campus::random_incident(rng, &campus, owner, 0);
        let mut alone = user_rng(5, 1);
        let plain: Vec<_> = (0..20).map(|_| draw(&mut alone)).collect();
        let mut paced = user_rng(5, 1);
        let mut p = Pacer::new(Pattern::Random, 1000, 5, 1);
        let with_waits: Vec<_> = (0..20)
            .map(|_| {
                p.next_wait(0);
                draw(&mut paced)
            })
            .collect();
        assert_eq!(plain, with_waits);
    }

    #[test]
    fn all_at_once_waits_land_on_the_next_multiple_of_the_think_time() {
        let mut p = pacer(Pattern::AllAtOnce, 1000);
        assert_eq!(p.next_wait(12_300), Duration::from_millis(700));
        assert_eq!(p.next_wait(12_999), Duration::from_millis(1));
        // Exactly on a multiple means the next one, so a user never sends twice in one moment.
        assert_eq!(p.next_wait(13_000), Duration::from_millis(1000));
        // The first send lines up too, whichever user it is: no spread.
        assert_eq!(p.first_wait(0, 100, 12_300), Duration::from_millis(700));
        assert_eq!(p.first_wait(99, 100, 12_300), Duration::from_millis(700));
    }

    fn parsed(extra: &[&str]) -> Args {
        let base = ["loadgen", "--client-id", "1"];
        Args::parse_from(base.iter().chain(extra))
    }

    #[test]
    fn a_pattern_that_needs_a_think_time_refuses_zero() {
        assert!(parsed(&["--pattern", "all-at-once"]).check().is_err());
        assert!(parsed(&["--pattern", "random"]).check().is_err());
        assert!(parsed(&["--pattern", "steady"]).check().is_ok());
        let think = ["--think-ms", "1000", "--pattern", "all-at-once"];
        assert!(parsed(&think).check().is_ok());
    }

    #[test]
    fn a_pattern_cannot_be_named_with_an_open_loop() {
        let named = [
            "loadgen",
            "--client-id",
            "1",
            "--rate",
            "5",
            "--pattern",
            "random",
        ];
        assert!(Args::try_parse_from(named).is_err());
        // The default pattern does not count as named.
        assert!(Args::try_parse_from(["loadgen", "--client-id", "1", "--rate", "5"]).is_ok());
    }

    #[test]
    fn the_summary_keeps_the_wording_the_start_script_adds_up() {
        let mut stats = Stats {
            latencies_us: vec![2000, 1000],
            gave_up: 3,
            retried: 4,
            no_responder: 5,
            provisional: 6,
            expired: 7,
        };
        let text = summary(10_000, &mut stats, Duration::from_secs(2), false);
        assert_eq!(
            text.lines().next(),
            Some(
                "client 10000: done 2, gave up 3, retried 4, no responder 5, provisional 6, \
                 expired 7"
            )
        );
        assert!(text.contains("throughput 1.0 req/s over 2.0 s"), "{text}");
        assert!(!text.contains("stopped"), "{text}");
        let early = summary(10_000, &mut stats, Duration::from_secs(2), true);
        assert!(early.contains("stopped by a signal"), "{early}");
    }
}
