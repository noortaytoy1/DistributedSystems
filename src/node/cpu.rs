//! Samples how busy this machine's CPU is, for the bid's `cpu_permille`. Reads the `cpu` line of
//! `/proc/stat` on a helper thread, because blocking file reads must stay off the async runtime.
//!
//! Owner: B (bidding election).

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;

use super::NodeContext;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(100);
const PROC_STAT: &str = "/proc/stat";

/// CPU time counters since boot, in the kernel's ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sample {
    busy: u64,
    total: u64,
}

/// Reads user, nice, system, idle, iowait, irq, softirq and steal from the `cpu` line. Idle and
/// iowait count as not busy.
fn parse(stat: &str) -> Option<Sample> {
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(|f| f.parse().ok())
        .collect::<Option<_>>()?;
    if fields.len() < 5 {
        return None;
    }
    let total = fields.iter().fold(0u64, |sum, f| sum.saturating_add(*f));
    let idle = fields[3].saturating_add(fields[4]);
    Some(Sample {
        busy: total.saturating_sub(idle),
        total,
    })
}

/// Busy share between two samples, 0..=1000. Zero if no time passed.
fn busy_permille(before: Sample, after: Sample) -> u32 {
    let total = after.total.saturating_sub(before.total);
    let busy = after.busy.saturating_sub(before.busy);
    if total == 0 {
        return 0;
    }
    (u128::from(busy.min(total)) * 1000 / u128::from(total)) as u32
}

/// Updates the node's reading every `SAMPLE_INTERVAL`. Where the file cannot be read the reading
/// stays 0, with one warning, and the task ends.
pub(super) fn spawn(ctx: Arc<NodeContext>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(SAMPLE_INTERVAL);
        let mut before = None;
        loop {
            tick.tick().await;
            let read = tokio::task::spawn_blocking(|| std::fs::read_to_string(PROC_STAT)).await;
            let Some(after) = read.ok().and_then(Result::ok).as_deref().and_then(parse) else {
                tracing::warn!("cannot read {PROC_STAT}; reporting CPU as 0");
                ctx.bids.set_cpu_permille(0);
                return;
            };
            if let Some(before) = before {
                ctx.bids.set_cpu_permille(busy_permille(before, after));
            }
            before = Some(after);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "cpu  100 20 30 800 50 0 0 0 0 0\ncpu0 1 2 3 4 5 6 7 8 9 10\n";

    #[test]
    fn parses_the_total_line_only() {
        assert_eq!(
            parse(STAT),
            Some(Sample {
                busy: 150,
                total: 1000
            })
        );
    }

    #[test]
    fn rejects_text_without_a_cpu_line() {
        assert_eq!(parse("intr 1 2 3\n"), None);
        assert_eq!(parse("cpu  1 2\n"), None);
        assert_eq!(parse("cpu  a b c d e\n"), None);
    }

    #[test]
    fn permille_is_the_busy_share_between_samples() {
        let before = Sample {
            busy: 100,
            total: 1000,
        };
        let after = Sample {
            busy: 350,
            total: 2000,
        };
        assert_eq!(busy_permille(before, after), 250);
    }

    #[test]
    fn permille_stays_in_range() {
        let before = Sample {
            busy: 100,
            total: 100,
        };
        assert_eq!(busy_permille(before, before), 0);
        let odd = Sample {
            busy: 900,
            total: 200,
        };
        assert_eq!(busy_permille(before, odd), 1000);
    }
}
