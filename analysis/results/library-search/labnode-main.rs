// labnode: one synevi node with a closed-loop load, to measure speed and failure behaviour.
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use synevi_core::node::Node;
use synevi_network::network::GrpcNetwork;
use synevi_persistence::database::PersistentStore;
use synevi_types::traits::Executor;
use synevi_types::types::{ExecutorResult, SyneviResult};
use ulid::Ulid;

#[derive(Default)]
struct Chain {
    state: Mutex<(u64, u64)>,
}

#[async_trait::async_trait]
impl Executor for Chain {
    type Tx = Vec<u8>;
    async fn execute(&self, tx: Vec<u8>) -> SyneviResult<Self> {
        let mut s = self.state.lock().unwrap();
        s.0 += 1;
        let mut h = if s.1 == 0 { 0xcbf2_9ce4_8422_2325 } else { s.1 };
        for b in &tx {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        s.1 = h;
        Ok(ExecutorResult::External(Ok(Vec::new())))
    }
}

fn arg(name: &str, default: &str) -> String {
    let flag = format!("--{name}");
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| *a == flag)
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| default.to_string())
}

fn name_of(id: usize) -> Ulid {
    Ulid::from_parts(1, id as u128 + 1)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() {
    let id: usize = arg("id", "0").parse().unwrap();
    let hosts: Vec<String> = arg("hosts", "").split(',').map(String::from).collect();
    let seconds: u64 = arg("seconds", "20").parse().unwrap();
    let conc: usize = arg("conc", "10").parse().unwrap();
    let bytes: usize = arg("bytes", "2000").parse().unwrap();
    let wait: u64 = arg("wait", "8").parse().unwrap();
    let disk = arg("disk", "");
    let join = arg("join", "");
    let port = hosts[id].rsplit(':').next().unwrap().to_string();
    let socket = SocketAddr::from_str(&format!("0.0.0.0:{port}")).unwrap();
    let network = GrpcNetwork::new(socket, format!("http://{}", hosts[id]), name_of(id), id as u16);
    let chain = Arc::new(Chain::default());
    let started = Instant::now();

    let store = PersistentStore::new(disk.clone(), id as u16).expect("store");
    let node = if join.is_empty() {
        let node = Node::new(name_of(id), id as u16, network, chain.clone(), store)
            .await
            .expect("node");
        for (other, host) in hosts.iter().enumerate() {
            if other == id {
                continue;
            }
            let mut tries = 0;
            while let Err(e) = node
                .add_member(name_of(other), other as u16, format!("http://{host}"), true)
                .await
            {
                tries += 1;
                assert!(tries < 120, "cannot reach {host}: {e}");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        node
    } else {
        let node = Node::new_with_member(
            name_of(id),
            id as u16,
            network,
            chain.clone(),
            store,
            format!("http://{join}"),
        )
        .await
        .expect("join");
        println!(
            "node {id} joined after {:.2} s, applied {}",
            started.elapsed().as_secs_f64(),
            chain.state.lock().unwrap().0
        );
        node
    };
    tokio::time::sleep(Duration::from_secs(wait)).await;

    let done = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let latencies = Arc::new(Mutex::new(Vec::<u32>::new()));
    let until = Instant::now() + Duration::from_secs(seconds);
    let mut workers = Vec::new();
    for w in 0..conc {
        let (node, done, errors, latencies) =
            (node.clone(), done.clone(), errors.clone(), latencies.clone());
        workers.push(tokio::spawn(async move {
            let mut seq = 0u64;
            while Instant::now() < until {
                seq += 1;
                let tx_id = ((id as u128 + 1) << 96) | ((w as u128) << 64) | u128::from(seq) | ((std::process::id() as u128) << 40);
                let mut payload = vec![(seq % 251) as u8; bytes];
                payload[..16].copy_from_slice(&tx_id.to_le_bytes());
                let t = Instant::now();
                match tokio::time::timeout(Duration::from_secs(30), node.clone().transaction(tx_id, payload)).await {
                    Ok(Ok(_)) => {
                        done.fetch_add(1, Ordering::Relaxed);
                        latencies.lock().unwrap().push(t.elapsed().as_micros() as u32);
                    }
                    _ => {
                        errors.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }));
    }
    let reporter = {
        let (done, errors, chain) = (done.clone(), errors.clone(), chain.clone());
        tokio::spawn(async move {
            let mut last = 0;
            let mut last_applied = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let d = done.load(Ordering::Relaxed);
                let (a, h) = *chain.state.lock().unwrap();
                println!("t{:.0} node {id}: done/s {} applied/s {} errors {} applied {} digest {:x}", started.elapsed().as_secs_f64(), d - last, a - last_applied, errors.load(Ordering::Relaxed), a, h);
                last = d;
                last_applied = a;
            }
        })
    };
    for w in workers {
        let _ = w.await;
    }
    // Let everything in flight finish, on all nodes.
    tokio::time::sleep(Duration::from_secs(wait)).await;
    reporter.abort();
    let mut l = latencies.lock().unwrap().clone();
    l.sort_unstable();
    let p = |q: f64| l.get(((l.len() as f64 * q) as usize).min(l.len().saturating_sub(1))).copied().unwrap_or(0) as f64 / 1000.0;
    let (a, h) = *chain.state.lock().unwrap();
    let (total, accepts, recovers) = node.get_stats();
    println!(
        "FINAL node {id}: own done {} ({:.0}/s) errors {} p50 {:.1} ms p99 {:.1} ms max {:.1} ms | applied {} digest {:x} | stats total {} accepts {} recovers {}",
        done.load(Ordering::Relaxed), done.load(Ordering::Relaxed) as f64 / seconds as f64, errors.load(Ordering::Relaxed), p(0.5), p(0.99), p(1.0), a, h, total, accepts, recovers
    );
    std::process::exit(0);
}
