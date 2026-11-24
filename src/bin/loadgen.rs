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
//! Phase 2 (D430): with `workload.lifecycle = true` in the config, each user also has a role and
//! registers, subscribes, discovers, talks to peers, offers, claims, moves zone and goes offline.
//! Offers and claims are stubs until Part J merges, counted as `stub` (D433, D436).
//!
//! Every address comes from `--config`, so the same command works on all three client machines.
//! Prints throughput and percentiles; raw events go to `metrics.dir` from the config. TERM or
//! Ctrl-C ends the run early and still prints the summary and flushes the log (D183).
//!
//! Owner: F (client and scale); the Phase 2 lifecycle: L.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, ValueEnum};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rescuemesh::campus;
use rescuemesh::config::{CampusConfig, Config, WorkloadConfig};
use rescuemesh::metrics::{self, Event, EventLog, now_ms};
use rescuemesh::middleware::{
    ClientError, Delivery, MeshClient, PeerError, PeerSend, PresenceError, Reconciled,
};
use rescuemesh::proto::{
    Capability, ClientId, Incident, IncidentStatus, IncidentSummary, P2pAck, P2pBody, Point,
    PresenceRecord, PresenceRole, RequestId, ResourceOffer, UserId, ZoneId,
};
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
    /// Take this program offline this many seconds into the run (Phase 2, D406). Reports made
    /// while offline wait in its outbox and are sent when it is back.
    #[arg(long, requires = "offline_for_s")]
    offline_at_s: Option<u64>,
    /// How long it stays offline.
    #[arg(long, requires = "offline_at_s")]
    offline_for_s: Option<u64>,
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

    /// What the Phase 2 lifecycle needs from the flags (D430, D432).
    fn check_workload(&self, w: &WorkloadConfig) -> anyhow::Result<()> {
        if !w.lifecycle {
            anyhow::ensure!(
                w.offline_chance_per_min_percent == 0,
                "workload.offline_chance_per_min_percent needs workload.lifecycle = true"
            );
            return Ok(());
        }
        anyhow::ensure!(
            self.rate.is_none(),
            "workload.lifecycle runs users in a closed loop: leave out --rate"
        );
        // Most actions are stubs that return at once, so with no think time a user would spin.
        anyhow::ensure!(
            self.think_ms > 0,
            "workload.lifecycle needs --think-ms above 0"
        );
        anyhow::ensure!(
            w.offline_chance_per_min_percent == 0 || self.offline_at_s.is_none(),
            "use either --offline-at-s or workload.offline_chance_per_min_percent, not both"
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
    /// Reports made while offline, which went into the outbox (D400).
    queued: u64,
    /// Phase 2 lifecycle actions, keyed `action:outcome` (D433). Empty when it is off.
    actions: BTreeMap<String, u64>,
}

impl Stats {
    fn record(&mut self, outcome: Result<Delivery, ClientError>) {
        match outcome {
            Ok(Delivery::Queued(_)) => self.queued += 1,
            Ok(Delivery::Answered(done)) => {
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
                let outcome = client
                    .submit_or_queue(incident(&mut rng, &campus, owner))
                    .await;
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
            let outcome = client.submit_or_queue_from(report, intended).await;
            if intended >= measure_from {
                lock(&stats).record(outcome);
            }
            drop(slot);
        });
        sent += 1;
    }
    while requests.join_next().await.is_some() {}
}

// ---------------------------------------------------------------------------------------------
// The Phase 2 lifecycle (Part L, D430 to D433). Off unless `workload.lifecycle` is true.
// ---------------------------------------------------------------------------------------------

/// What a simulated user is in Phase 2 (D431).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Reports incidents, looks up who is around, talks to peers.
    Ordinary,
    /// A volunteer responder: claims response tasks.
    Responder,
    /// Offers resources for others to claim.
    Provider,
}

/// One thing a user does (D430).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Join the directory (Part G, through Part H's presence agent). Once, at the start.
    Register,
    /// Subscribe to the home zone (Part I). Once, at the start.
    Subscribe,
    /// Report an incident to the cloud: the Phase 1 path, with Part K's outbox while offline.
    Report,
    /// Ask the directory who is in a zone or offers a capability (Part G).
    Discover,
    /// Send an update straight to a discovered peer (Part H).
    PeerSend,
    /// Offer or renew a resource (Part J).
    Offer,
    /// Claim a resource or a response task (Part J).
    Claim,
    /// Move to another zone (Part G's zone change, Part I's subscription).
    MoveZone,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Action::Register => "register",
            Action::Subscribe => "subscribe",
            Action::Report => "report",
            Action::Discover => "discover",
            Action::PeerSend => "peer_send",
            Action::Offer => "offer",
            Action::Claim => "claim",
            Action::MoveZone => "move_zone",
        }
    }
}

/// After registering and subscribing, what each role does per think time, in percent (D431). Each
/// row adds up to 100. Ordinary users still report most, so the cloud keeps its Phase 1 load.
const ORDINARY_ACTIONS: [(Action, u8); 4] = [
    (Action::Report, 60),
    (Action::Discover, 20),
    (Action::PeerSend, 15),
    (Action::MoveZone, 5),
];
const RESPONDER_ACTIONS: [(Action, u8); 4] = [
    (Action::Claim, 40),
    (Action::Discover, 30),
    (Action::PeerSend, 25),
    (Action::MoveZone, 5),
];
const PROVIDER_ACTIONS: [(Action, u8); 4] = [
    (Action::Offer, 40),
    (Action::PeerSend, 30),
    (Action::Discover, 25),
    (Action::MoveZone, 5),
];

/// Kept apart from the incident and pacing streams, so turning the lifecycle on never changes
/// which incidents a seed draws.
const LIFE_STREAM: u64 = 0x4C49_4645;
const OUTAGE_STREAM: u64 = 0x4F55_5447;

/// The role of user `user` of `users` (D431). Each user owns an equal slice of 0..100, and the role
/// whose share holds the slice's middle gets it. A program's roles then match the shares as closely
/// as its user count allows, with no draw: 100 users give exactly 80, 15 and 5, and 10 users 8, 1 and 1.
fn role_of(user: u32, users: u32, w: &WorkloadConfig) -> Role {
    let middle = (2 * u64::from(user) + 1) * 100 / (2 * u64::from(users.max(1)));
    let ordinary = u64::from(w.ordinary_percent);
    if middle < ordinary {
        Role::Ordinary
    } else if middle < ordinary + u64::from(w.responder_percent) {
        Role::Responder
    } else {
        Role::Provider
    }
}

/// How many users of `users` get each role: (ordinary, responders, providers).
fn role_counts(users: u32, w: &WorkloadConfig) -> (u32, u32, u32) {
    (0..users).fold((0, 0, 0), |(o, r, p), user| match role_of(user, users, w) {
        Role::Ordinary => (o + 1, r, p),
        Role::Responder => (o, r + 1, p),
        Role::Provider => (o, r, p + 1),
    })
}

/// One user's Phase 2 life: its role, its zone, what it can do, the peers it last found, and its own
/// generator (D430).
struct Life {
    role: Role,
    zone: ZoneId,
    /// What a responder or provider offers; none for an ordinary user.
    capability: Option<Capability>,
    /// Users of other programs from this user's last discovery: whom a peer send goes to (D436).
    found: Vec<PresenceRecord>,
    rng: StdRng,
}

impl Life {
    fn new(
        seed: u64,
        client: ClientId,
        user: u32,
        users: u32,
        w: &WorkloadConfig,
        zones: u16,
    ) -> Self {
        let stream = seed.wrapping_mul(1_000_003).wrapping_add(u64::from(user))
            ^ client.rotate_left(32)
            ^ LIFE_STREAM;
        let mut rng = StdRng::seed_from_u64(stream);
        let zone = rng.gen_range(0..zones.max(1));
        let role = role_of(user, users, w);
        let capability = Capability::ALL[rng.gen_range(0..Capability::ALL.len())];
        Self {
            role,
            zone,
            capability: (role != Role::Ordinary).then_some(capability),
            found: Vec::new(),
            rng,
        }
    }

    /// The next action, drawn from this role's row.
    fn next_action(&mut self) -> Action {
        let row = match self.role {
            Role::Ordinary => &ORDINARY_ACTIONS,
            Role::Responder => &RESPONDER_ACTIONS,
            Role::Provider => &PROVIDER_ACTIONS,
        };
        let mut draw = self.rng.gen_range(0..100u8);
        for &(action, percent) in row {
            if draw < percent {
                return action;
            }
            draw -= percent;
        }
        row[0].0
    }

    /// Moves to another zone, if the campus has more than one.
    fn move_zone(&mut self, zones: u16) {
        if zones > 1 {
            self.zone = (self.zone + self.rng.gen_range(1..zones)) % zones;
        }
    }

    /// The directory's word for this role.
    fn presence_role(&self) -> PresenceRole {
        match self.role {
            Role::Ordinary => PresenceRole::Ordinary,
            Role::Responder => PresenceRole::VolunteerResponder,
            Role::Provider => PresenceRole::ResourceProvider,
        }
    }

    /// A peer from the last discovery, at random. `None` if it found nobody in another program.
    fn pick_peer(&mut self) -> Option<PresenceRecord> {
        (!self.found.is_empty())
            .then(|| self.found[self.rng.gen_range(0..self.found.len())].clone())
    }
}

/// What every user of one program shares for the Phase 2 calls.
struct Shared {
    client: Arc<MeshClient>,
    zones: u16,
    /// The last incident this program reported: what an ordinary user or a responder passes to a
    /// peer, so a peer update is about a real report (D436).
    last_report: Mutex<Option<IncidentSummary>>,
    /// A place in each zone, taken from the campus's first responder there: where a provider says
    /// its offer is.
    spots: Vec<Point>,
}

impl Shared {
    fn new(client: Arc<MeshClient>, campus: &CampusConfig) -> Self {
        let mut spots = vec![Point { x: 0, y: 0 }; usize::from(campus.zones.max(1))];
        for r in campus::responders(campus).iter().rev() {
            if let Some(spot) = spots.get_mut(usize::from(r.zone)) {
                *spot = r.location;
            }
        }
        Self {
            client,
            zones: campus.zones,
            last_report: Mutex::new(None),
            spots,
        }
    }

    fn spot(&self, zone: ZoneId) -> Point {
        self.spots
            .get(usize::from(zone))
            .copied()
            .unwrap_or(Point { x: 0, y: 0 })
    }
}

/// An outcome name for a presence call (D436). `rejected` is what every node answers until Part G's
/// directory merges (D325).
fn presence_outcome<T>(result: &Result<T, PresenceError>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(PresenceError::Rejected(_)) => "rejected",
        Err(PresenceError::GaveUp { .. }) => "gave_up",
        Err(PresenceError::Offline) => "offline",
        Err(PresenceError::NotJoined(_)) => "not_joined",
        Err(PresenceError::NoPeerEndpoint) => "no_endpoint",
    }
}

/// An outcome name for a peer send (D436).
fn peer_outcome(result: &Result<PeerSend, PeerError>) -> &'static str {
    match result {
        Ok(PeerSend::Delivered(d)) => match d.ack {
            P2pAck::Accepted => "delivered",
            P2pAck::Duplicate => "duplicate",
            P2pAck::Refused(_) => "refused",
        },
        Ok(PeerSend::Held) => "held",
        Err(PeerError::Offline) => "offline",
        Err(PeerError::GaveUp { .. }) => "gave_up",
        Err(_) => "failed",
    }
}

/// A Phase 2 call other than a report (D433, D436). Join, discovery, moving zone and peer sends are
/// Part H's real calls; offers and claims stay stubs until Part J merges, sending nothing and
/// counted as `stub`, so no number in the analysis can come from them.
async fn phase2_call(
    action: Action,
    life: &mut Life,
    owner: UserId,
    shared: &Shared,
) -> &'static str {
    let client = &shared.client;
    match action {
        Action::Register => {
            let capabilities = life.capability.into_iter().collect();
            presence_outcome(
                &client
                    .join(owner, life.presence_role(), life.zone, capabilities)
                    .await,
            )
        }
        // The program subscribes to the zones of its joined users (Part I, D359): joining is the
        // subscription, so this counts what the join did.
        Action::Subscribe => "with_join",
        Action::Discover => {
            // By zone, or by capability half the time for a user who has one.
            let found = match life.capability {
                Some(capability) if life.rng.gen_bool(0.5) => {
                    client.discover_capability(capability).await
                }
                _ => client.discover_zone(life.zone).await,
            };
            if let Ok(found) = &found {
                life.found = found
                    .records
                    .iter()
                    .filter(|r| r.user.client != owner.client)
                    .cloned()
                    .collect();
            }
            presence_outcome(&found)
        }
        Action::PeerSend => {
            let Some(peer) = life.pick_peer() else {
                return "no_peer";
            };
            let body = match life.role {
                Role::Provider => P2pBody::ResourceOffer(ResourceOffer {
                    provider: owner,
                    capability: life.capability.unwrap_or(Capability::ALL[0]),
                    zone: life.zone,
                    location: shared.spot(life.zone),
                    available_until_ms: now_ms() + OFFER_FOR_MS,
                }),
                Role::Ordinary | Role::Responder => {
                    let Some(summary) = lock(&shared.last_report).clone() else {
                        return "nothing_to_send";
                    };
                    P2pBody::IncidentSummary(summary)
                }
            };
            let update = client.new_update(owner, Some(peer.user), peer.zone, 1, 0, body);
            peer_outcome(&client.send_peer_or_hold(&peer.peer_addr, update).await)
        }
        // TODO(L): offer and claim when Part J merges.
        Action::Offer | Action::Claim => "stub",
        Action::MoveZone => {
            life.move_zone(shared.zones);
            presence_outcome(&client.move_zone(owner, life.zone).await)
        }
        // Reports go through `lifecycle_user`, never here.
        Action::Report => "not_a_phase2_call",
    }
}

/// How long a provider's offer to a peer says it holds.
const OFFER_FOR_MS: u64 = 60_000;

/// One user's Phase 2 loop: register, subscribe, then one action per think time as its role says,
/// until the run ends. Reports are real and counted as in Phase 1.
#[allow(clippy::too_many_arguments)]
async fn lifecycle_user(
    shared: Arc<Shared>,
    campus: CampusConfig,
    mut life: Life,
    mut rng: StdRng,
    mut pacer: Pacer,
    owner: UserId,
    users: u32,
    measure_from: Instant,
    stop_at: Instant,
    stats: Arc<Mutex<Stats>>,
) {
    let count = |stats: &Arc<Mutex<Stats>>, started: Instant, action: Action, outcome: &str| {
        if started >= measure_from {
            *lock(stats)
                .actions
                .entry(format!("{}:{outcome}", action.name()))
                .or_default() += 1;
        }
    };
    for action in [Action::Register, Action::Subscribe] {
        let started = Instant::now();
        let outcome = phase2_call(action, &mut life, owner, &shared).await;
        count(&stats, started, action, outcome);
    }
    pause(pacer.first_wait(owner.user, users, now_ms()), stop_at).await;
    while Instant::now() < stop_at {
        let started = Instant::now();
        let action = life.next_action();
        if action == Action::Report {
            let report = incident(&mut rng, &campus, owner);
            let outcome = shared.client.submit_or_queue(report.clone()).await;
            let id = match &outcome {
                Ok(Delivery::Answered(done)) => Some(done.id),
                Ok(Delivery::Queued(id)) => Some(*id),
                Err(_) => None,
            };
            if let Some(id) = id {
                *lock(&shared.last_report) = Some(summary_of(id, &report));
            }
            let shown = match &outcome {
                Ok(Delivery::Answered(_)) => "answered",
                Ok(Delivery::Queued(_)) => "queued",
                Err(ClientError::GaveUp { .. }) => "gave_up",
                Err(_) => "failed",
            };
            count(&stats, started, action, shown);
            if started >= measure_from {
                lock(&stats).record(outcome);
            }
        } else {
            let outcome = phase2_call(action, &mut life, owner, &shared).await;
            count(&stats, started, action, outcome);
        }
        pause(pacer.next_wait(now_ms()), stop_at).await;
    }
}

/// A report as peers pass it on.
fn summary_of(id: RequestId, report: &Incident) -> IncidentSummary {
    IncidentSummary {
        incident: id,
        category: report.category,
        severity: report.severity,
        zone: report.zone,
        location: report.location,
        needs: report.needs,
        reported_at_ms: report.reported_at_ms,
    }
}

/// Reads what other programs deliver to this one, so the inbox never fills, and counts it by kind
/// as `peer_received:<kind>` (D436). Ends when the client is dropped.
async fn drain_peer_updates(
    client: Arc<MeshClient>,
    measure_from: Instant,
    stats: Arc<Mutex<Stats>>,
) {
    let Some(mut updates) = client.take_peer_updates() else {
        return;
    };
    drop(client);
    while let Some(update) = updates.recv().await {
        if Instant::now() >= measure_from {
            let kind = match update.body {
                P2pBody::IncidentSummary(_) => "incident_summary",
                P2pBody::ResourceOffer(_) => "resource_offer",
                P2pBody::AssignmentUpdate(_) => "assignment_update",
            };
            *lock(&stats)
                .actions
                .entry(format!("peer_received:{kind}"))
                .or_default() += 1;
        }
    }
}

/// The closed loop with the Phase 2 lifecycle: every user lives its role (D430).
async fn lifecycle_loop(
    args: &Args,
    workload: WorkloadConfig,
    client: Arc<MeshClient>,
    campus: CampusConfig,
    measure_from: Instant,
    stop_at: Instant,
    stats: Arc<Mutex<Stats>>,
) {
    let total = args.users;
    let drain = tokio::spawn(drain_peer_updates(
        client.clone(),
        measure_from,
        stats.clone(),
    ));
    let shared = Arc::new(Shared::new(client, &campus));
    let mut users = JoinSet::new();
    for user in 0..total {
        let life = Life::new(
            args.seed,
            args.client_id,
            user,
            total,
            &workload,
            campus.zones,
        );
        let owner = UserId {
            client: args.client_id,
            user,
        };
        users.spawn(lifecycle_user(
            shared.clone(),
            campus,
            life,
            user_rng(args.seed, u64::from(user)),
            Pacer::new(args.pattern, args.think_ms, args.seed, u64::from(user)),
            owner,
            total,
            measure_from,
            stop_at,
            stats.clone(),
        ));
    }
    while users.join_next().await.is_some() {}
    // What arrived after the users stopped is not counted.
    drain.abort();
}

/// When this program's random outages start, as offsets from the run's start (D432). Each whole
/// minute of the run draws once: with `chance` percent the program goes offline at a random moment
/// of that minute, early enough to be back before the minute ends. Seeded, so a run repeats.
fn outage_starts(
    seed: u64,
    client: ClientId,
    run: Duration,
    chance: u8,
    length: Duration,
) -> Vec<Duration> {
    let mut rng = StdRng::seed_from_u64(seed ^ client.rotate_left(32) ^ OUTAGE_STREAM);
    let minute = Duration::from_secs(60);
    let latest = minute.saturating_sub(length).as_millis() as u64;
    (0..run.as_secs() / 60)
        .filter_map(|m| {
            let hit = rng.gen_range(0..100u8) < chance;
            let at = minute * m as u32 + Duration::from_millis(rng.gen_range(0..=latest));
            (hit && at + length <= run).then_some(at)
        })
        .collect()
}

/// What this program's random outages added up to (D432).
#[derive(Debug, Default, Clone, PartialEq)]
struct Outages {
    count: u32,
    sent: usize,
    answered: usize,
    expired: usize,
    failed: usize,
    longest_reconcile: Duration,
}

/// Runs the outages `outage_starts` drew, one after another, and adds up each reconnection.
async fn random_outages(
    client: Arc<MeshClient>,
    start: Instant,
    starts: Vec<Duration>,
    length: Duration,
    total: Arc<Mutex<Outages>>,
) {
    for at in starts {
        let flushed = go_offline_once(client.clone(), start + at, length).await;
        let mut t = lock(&total);
        t.count += 1;
        if let Some(done) = flushed {
            t.sent += done.sent;
            t.answered += done.answered;
            t.expired += done.expired;
            t.failed += done.failed;
            t.longest_reconcile = t.longest_reconcile.max(done.took);
        }
    }
}

/// One more line after the summary for a lifecycle run. Not on the first line, which
/// `scripts/start-users.sh` adds up.
fn lifecycle_summary(
    roles: (u32, u32, u32),
    actions: &BTreeMap<String, u64>,
    outages: &Outages,
) -> String {
    let listed = actions
        .iter()
        .map(|(key, n)| format!("{key} {n}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "phase2: users ordinary {}, responders {}, providers {}; actions {}; outages {}, sent {}, \
         answered {}, expired {}, failed {}, longest reconcile {:.1} ms\n",
        roles.0,
        roles.1,
        roles.2,
        if listed.is_empty() { "none" } else { &listed },
        outages.count,
        outages.sent,
        outages.answered,
        outages.expired,
        outages.failed,
        outages.longest_reconcile.as_secs_f64() * 1000.0,
    )
}

/// Takes the client offline at `at` for `length`, then brings it back and sends its outbox (D406).
/// `None` if the run ends first or the flush fails, which is logged.
async fn go_offline_once(
    client: Arc<MeshClient>,
    at: Instant,
    length: Duration,
) -> Option<Reconciled> {
    tokio::time::sleep_until(at).await;
    client.go_offline();
    tokio::time::sleep(length).await;
    match client.go_online().await {
        Ok(done) => Some(done),
        Err(e) => {
            tracing::warn!("sending the outbox failed: {e}");
            None
        }
    }
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

/// One more line after the summary, only for a run with an outage. Not on the first line, which
/// `scripts/start-users.sh` adds up.
fn offline_summary(queued: u64, flushed: Option<Reconciled>, still_queued: usize) -> String {
    match flushed {
        Some(done) => format!(
            "offline: queued {queued}, sent {}, answered {}, expired {}, failed {}, \
             reconciled in {:.1} ms, still queued {still_queued}\n",
            done.sent,
            done.answered,
            done.expired,
            done.failed,
            done.took.as_secs_f64() * 1000.0,
        ),
        None => format!(
            "offline: queued {queued}, not reconciled before the run ended, still queued \
             {still_queued}\n"
        ),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rescuemesh::init_tracing();
    let args = Args::parse();
    let config =
        Config::load(&args.config).with_context(|| format!("loading {}", args.config.display()))?;
    args.check()?;
    let workload = config.workload;
    args.check_workload(&workload)?;
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
    let first_node = config.cluster.nodes.first().map(|n| n.client_addr.as_str());
    let (host, ip) = rescuemesh::metrics::this_pc(first_node);
    log.log(Event::ClientStarted {
        users: args.rate.map_or(args.users, |_| 0),
        host,
        ip,
    });
    let measure_from = start + Duration::from_secs(args.warmup_s);
    let stop_at = start + Duration::from_secs(args.duration_s);
    let shared = stats.clone();
    let mut campus = config.campus;
    if let Some(ttl_ms) = args.ttl_ms {
        campus.incident_ttl_ms = ttl_ms;
    }
    let outage = args
        .offline_at_s
        .zip(args.offline_for_s)
        .map(|(at, length)| {
            tokio::spawn(go_offline_once(
                client.clone(),
                start + Duration::from_secs(at),
                Duration::from_secs(length),
            ))
        });
    let roles = role_counts(args.users, &workload);
    let outages = Arc::new(Mutex::new(Outages::default()));
    let mut outage_task = None;
    if workload.lifecycle {
        log.log(Event::WorkloadStarted {
            ordinary: roles.0,
            responders: roles.1,
            providers: roles.2,
            offline_chance_per_min_percent: workload.offline_chance_per_min_percent,
            offline_for_ms: workload.offline_for_ms,
        });
        let length = Duration::from_millis(workload.offline_for_ms);
        let starts = outage_starts(
            args.seed,
            args.client_id,
            Duration::from_secs(args.duration_s),
            workload.offline_chance_per_min_percent,
            length,
        );
        if !starts.is_empty() {
            outage_task = Some(tokio::spawn(random_outages(
                client.clone(),
                start,
                starts,
                length,
                outages.clone(),
            )));
        }
    }
    let run = async {
        match args.rate {
            None if workload.lifecycle => {
                lifecycle_loop(
                    &args,
                    workload,
                    client.clone(),
                    campus,
                    measure_from,
                    stop_at,
                    shared,
                )
                .await;
            }
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
    let mut text = summary(args.client_id, &mut lock(&stats), measured, stopped_early);
    if let Some(outage) = outage {
        // An outage still running at the end is left there: its outbox stays on disk for the next
        // start with this client ID.
        let flushed = if outage.is_finished() {
            outage.await.ok().flatten()
        } else {
            outage.abort();
            None
        };
        let queued = lock(&stats).queued;
        let still_queued = client.queued().await;
        text.push_str(&offline_summary(queued, flushed, still_queued));
    }
    if workload.lifecycle {
        // An outage still on at the end stays as it is, as with `--offline-at-s`: its outbox
        // waits on disk for the next start with this client ID.
        if let Some(task) = outage_task {
            task.abort();
            let _ = task.await;
        }
        let outages = lock(&outages).clone();
        let actions = std::mem::take(&mut lock(&stats).actions);
        text.push_str(&lifecycle_summary(roles, &actions, &outages));
        log.log(Event::WorkloadDone {
            actions,
            outages: outages.count,
        });
    }
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
            queued: 8,
            actions: BTreeMap::new(),
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

    fn workload(lifecycle: bool, chance: u8) -> WorkloadConfig {
        WorkloadConfig {
            lifecycle,
            offline_chance_per_min_percent: chance,
            ..WorkloadConfig::default()
        }
    }

    #[test]
    fn roles_match_the_shares_as_closely_as_the_user_count_allows() {
        let w = WorkloadConfig::default();
        assert_eq!(role_counts(100, &w), (80, 15, 5));
        assert_eq!(role_counts(1000, &w), (800, 150, 50));
        assert_eq!(role_counts(10, &w), (8, 1, 1));
        assert_eq!(role_counts(1, &w), (1, 0, 0));
        assert_eq!(role_counts(0, &w), (0, 0, 0));
        let all_providers = WorkloadConfig {
            ordinary_percent: 0,
            responder_percent: 0,
            provider_percent: 100,
            ..w
        };
        assert_eq!(role_counts(7, &all_providers), (0, 0, 7));
    }

    #[test]
    fn every_role_row_adds_up_to_100() {
        for row in [ORDINARY_ACTIONS, RESPONDER_ACTIONS, PROVIDER_ACTIONS] {
            assert_eq!(row.iter().map(|&(_, p)| u32::from(p)).sum::<u32>(), 100);
        }
    }

    fn life(role_user: u32, users: u32, seed: u64, client: ClientId) -> Life {
        Life::new(
            seed,
            client,
            role_user,
            users,
            &WorkloadConfig::default(),
            6,
        )
    }

    #[test]
    fn each_role_does_what_its_row_says() {
        let tally = |mut l: Life| {
            let mut n = BTreeMap::new();
            for _ in 0..10_000 {
                *n.entry(l.next_action().name()).or_insert(0u32) += 1;
            }
            n
        };
        // Of 100 users, 0 is ordinary, 85 a responder and 97 a provider.
        let ordinary = tally(life(0, 100, 5, 1));
        let responder = tally(life(85, 100, 5, 1));
        let provider = tally(life(97, 100, 5, 1));
        assert!((5_700..6_300).contains(&ordinary["report"]), "{ordinary:?}");
        assert!(!ordinary.contains_key("claim") && !ordinary.contains_key("offer"));
        assert!(!responder.contains_key("report") && !responder.contains_key("offer"));
        assert!(
            (3_700..4_300).contains(&responder["claim"]),
            "{responder:?}"
        );
        assert!(!provider.contains_key("report") && !provider.contains_key("claim"));
        assert!((3_700..4_300).contains(&provider["offer"]), "{provider:?}");
    }

    #[test]
    fn a_life_repeats_for_a_seed_and_differs_between_programs() {
        let actions = |mut l: Life| (l.zone, (0..50).map(|_| l.next_action()).collect::<Vec<_>>());
        assert_eq!(actions(life(3, 10, 5, 1)), actions(life(3, 10, 5, 1)));
        assert_ne!(actions(life(3, 10, 5, 1)), actions(life(3, 10, 5, 2)));
        assert_ne!(actions(life(3, 10, 5, 1)), actions(life(3, 10, 6, 1)));
    }

    #[test]
    fn moving_always_lands_in_another_zone_of_the_campus() {
        let mut l = life(0, 1, 5, 1);
        for _ in 0..200 {
            let before = l.zone;
            l.move_zone(6);
            assert_ne!(l.zone, before);
            assert!(l.zone < 6);
        }
        l.zone = 0;
        l.move_zone(1);
        assert_eq!(l.zone, 0, "one zone: nowhere to go");
    }

    #[test]
    fn only_responders_and_providers_offer_a_capability() {
        assert_eq!(life(0, 100, 5, 1).capability, None);
        assert!(life(85, 100, 5, 1).capability.is_some());
        assert!(life(97, 100, 5, 1).capability.is_some());
        assert_eq!(
            life(85, 100, 5, 1).presence_role(),
            PresenceRole::VolunteerResponder
        );
        assert_eq!(
            life(97, 100, 5, 1).presence_role(),
            PresenceRole::ResourceProvider
        );
    }

    #[test]
    fn a_peer_comes_from_the_last_discovery_or_not_at_all() {
        let mut l = life(0, 1, 5, 1);
        assert_eq!(l.pick_peer(), None);
        let record = PresenceRecord {
            user: UserId { client: 2, user: 3 },
            role: PresenceRole::Ordinary,
            zone: 1,
            peer_addr: "10.0.0.2:20000".into(),
            capabilities: vec![],
            incarnation: 1,
        };
        l.found = vec![record.clone()];
        assert_eq!(l.pick_peer(), Some(record));
    }

    #[test]
    fn outcomes_name_what_happened_and_no_directory_reads_as_rejected() {
        let rejected: Result<(), _> = Err(PresenceError::Rejected("no directory".into()));
        assert_eq!(presence_outcome(&rejected), "rejected");
        assert_eq!(presence_outcome(&Ok::<_, PresenceError>(())), "ok");
        assert_eq!(
            presence_outcome::<()>(&Err(PresenceError::Offline)),
            "offline"
        );
        let delivered = |ack| {
            Ok(PeerSend::Delivered(rescuemesh::middleware::Delivered {
                ack,
                latency: Duration::from_millis(1),
                tries: 1,
            }))
        };
        assert_eq!(peer_outcome(&delivered(P2pAck::Accepted)), "delivered");
        assert_eq!(peer_outcome(&delivered(P2pAck::Duplicate)), "duplicate");
        assert_eq!(peer_outcome(&Ok(PeerSend::Held)), "held");
        assert_eq!(
            peer_outcome(&Err(PeerError::GaveUp { tries: 3 })),
            "gave_up"
        );
    }

    #[test]
    fn outages_repeat_for_a_seed_and_end_inside_their_minute_and_the_run() {
        let length = Duration::from_secs(20);
        let run = Duration::from_secs(600);
        assert!(outage_starts(5, 1, run, 0, length).is_empty());
        let every = outage_starts(5, 1, run, 100, length);
        assert_eq!(every.len(), 10, "one per whole minute at 100%");
        for (m, at) in every.iter().enumerate() {
            let minute = Duration::from_secs(60 * m as u64);
            assert!(
                *at >= minute && *at + length <= minute + Duration::from_secs(60),
                "{at:?}"
            );
        }
        assert_eq!(
            outage_starts(5, 1, run, 30, length),
            outage_starts(5, 1, run, 30, length)
        );
        assert_ne!(
            outage_starts(5, 1, run, 30, length),
            outage_starts(5, 2, run, 30, length)
        );
        // A run shorter than a minute has no whole minute to draw in.
        assert!(outage_starts(5, 1, Duration::from_secs(59), 100, length).is_empty());
    }

    #[test]
    fn the_lifecycle_refuses_flags_it_cannot_run_with() {
        let closed = parsed(&["--think-ms", "1000"]);
        assert!(closed.check_workload(&workload(true, 10)).is_ok());
        assert!(closed.check_workload(&workload(false, 0)).is_ok());
        assert!(
            closed.check_workload(&workload(false, 10)).is_err(),
            "outages need the lifecycle"
        );
        assert!(
            parsed(&[]).check_workload(&workload(true, 0)).is_err(),
            "no think time"
        );
        let open = parsed(&["--rate", "100", "--think-ms", "1000"]);
        assert!(open.check_workload(&workload(true, 0)).is_err());
        assert!(
            open.check_workload(&workload(false, 0)).is_ok(),
            "Phase 1 open loop is unchanged"
        );
        let both = [
            "--think-ms",
            "1000",
            "--offline-at-s",
            "5",
            "--offline-for-s",
            "5",
        ];
        assert!(parsed(&both).check_workload(&workload(true, 10)).is_err());
        assert!(parsed(&both).check_workload(&workload(true, 0)).is_ok());
    }

    #[test]
    fn the_lifecycle_line_lists_roles_actions_and_outages() {
        let mut actions = BTreeMap::new();
        actions.insert("discover:stub".to_string(), 3);
        actions.insert("report:answered".to_string(), 7);
        let outages = Outages {
            count: 2,
            sent: 4,
            answered: 3,
            expired: 1,
            failed: 0,
            longest_reconcile: Duration::from_millis(12),
        };
        assert_eq!(
            lifecycle_summary((8, 1, 1), &actions, &outages),
            "phase2: users ordinary 8, responders 1, providers 1; actions discover:stub 3, \
             report:answered 7; outages 2, sent 4, answered 3, expired 1, failed 0, longest \
             reconcile 12.0 ms\n"
        );
        assert!(
            lifecycle_summary((0, 0, 0), &BTreeMap::new(), &Outages::default())
                .contains("actions none")
        );
    }

    #[test]
    fn an_outage_adds_a_line_and_needs_both_flags() {
        let done = Reconciled {
            sent: 5,
            answered: 4,
            expired: 1,
            failed: 0,
            took: Duration::from_millis(12),
        };
        assert_eq!(
            offline_summary(5, Some(done), 0),
            "offline: queued 5, sent 5, answered 4, expired 1, failed 0, reconciled in 12.0 ms, \
             still queued 0\n"
        );
        assert!(offline_summary(3, None, 3).contains("not reconciled"));
        let one = ["loadgen", "--client-id", "1", "--offline-at-s", "2"];
        assert!(Args::try_parse_from(one).is_err());
    }
}
