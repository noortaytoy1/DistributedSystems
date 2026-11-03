//! Reads this process's memory and CPU and the machine's network counters from `/proc`, for the
//! once-a-second `resource_usage` event (D128). The spec asks for CPU, memory, network and queues.
//! UDP datagrams the kernel threw away come from here too (D171).
//!
//! Blocking file reads, so the caller runs [`read`] off the async runtime. A file that cannot be
//! read leaves its fields out; the parsers are pure, so they are easy to test.
//!
//! Owner: shared (measurement).

/// What the kernel reports right now. Counters are cumulative; the analysis takes differences.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub rss_kb: Option<u64>,
    /// User plus system CPU time of this process, in kernel ticks (`USER_HZ`, 100 per second on
    /// Ubuntu).
    pub cpu_ticks: Option<u64>,
    /// All interfaces but loopback. Includes other traffic on the machine, so keep it quiet.
    pub net_rx_bytes: Option<u64>,
    pub net_tx_bytes: Option<u64>,
    /// The machine's `UdpRcvbufErrors`: datagrams dropped because a socket's receive buffer was
    /// full. All of this machine's sockets, so on one machine it counts all three nodes.
    pub udp_drops: Option<u64>,
}

pub fn read() -> Usage {
    let file = |path| std::fs::read_to_string(path).ok();
    let (net_rx_bytes, net_tx_bytes) = file("/proc/net/dev").as_deref().and_then(parse_net).unzip();
    Usage {
        rss_kb: file("/proc/self/status").as_deref().and_then(parse_rss_kb),
        cpu_ticks: file("/proc/self/stat").as_deref().and_then(parse_cpu_ticks),
        net_rx_bytes,
        net_tx_bytes,
        udp_drops: read_udp_drops(),
    }
}

/// Only the drop count, for client programs, which log nothing else from here.
pub fn read_udp_drops() -> Option<u64> {
    let snmp = std::fs::read_to_string("/proc/net/snmp").ok()?;
    parse_udp_drops(&snmp)
}

fn parse_rss_kb(status: &str) -> Option<u64> {
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// `utime` and `stime` are fields 14 and 15 of `/proc/self/stat`. The process name in field 2 may
/// contain spaces, so counting starts after its closing bracket, at field 3.
fn parse_cpu_ticks(stat: &str) -> Option<u64> {
    let (_, rest) = stat.rsplit_once(')')?;
    let mut fields = rest.split_whitespace().skip(11);
    let utime: u64 = fields.next()?.parse().ok()?;
    let stime: u64 = fields.next()?.parse().ok()?;
    Some(utime.saturating_add(stime))
}

/// Received bytes are the first number after the interface name, sent bytes the ninth.
fn parse_net(dev: &str) -> Option<(u64, u64)> {
    let mut totals = None;
    for line in dev.lines().skip(2) {
        let (name, numbers) = line.split_once(':')?;
        if name.trim() == "lo" {
            totals = totals.or(Some((0, 0)));
            continue;
        }
        let fields: Vec<&str> = numbers.split_whitespace().collect();
        let rx: u64 = fields.first()?.parse().ok()?;
        let tx: u64 = fields.get(8)?.parse().ok()?;
        let (sum_rx, sum_tx) = totals.unwrap_or((0u64, 0u64));
        totals = Some((sum_rx.saturating_add(rx), sum_tx.saturating_add(tx)));
    }
    totals
}

/// `/proc/net/snmp` gives each protocol a line of names, then a line of numbers. Found by name,
/// because kernels add columns, and "Udp:" with its colon, because "UdpLite:" follows it.
fn parse_udp_drops(snmp: &str) -> Option<u64> {
    let mut lines = snmp.lines().filter(|l| l.starts_with("Udp:"));
    let names = lines.next()?;
    let values = lines.next()?;
    let column = names
        .split_whitespace()
        .position(|name| name == "RcvbufErrors")?;
    values.split_whitespace().nth(column)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udp_drops_come_from_the_udp_lines_by_name() {
        let snmp = "Tcp: RtoAlgorithm RcvbufErrors\nTcp: 1 7\n\
            Udp: InDatagrams NoPorts InErrors OutDatagrams RcvbufErrors SndbufErrors\n\
            Udp: 100 2 3 90 41 0\n\
            UdpLite: InDatagrams NoPorts InErrors OutDatagrams RcvbufErrors SndbufErrors\n\
            UdpLite: 0 0 0 0 5 0\n";
        assert_eq!(parse_udp_drops(snmp), Some(41));
        assert_eq!(parse_udp_drops("Tcp: A\nTcp: 1\n"), None);
        assert_eq!(parse_udp_drops("Udp: InDatagrams\nUdp: 1\n"), None);
    }

    #[test]
    fn memory_comes_from_vmrss() {
        let status = "Name:\tnode\nVmPeak:\t  9000 kB\nVmRSS:\t  1234 kB\nThreads:\t4\n";
        assert_eq!(parse_rss_kb(status), Some(1234));
        assert_eq!(parse_rss_kb("Name:\tnode\n"), None);
    }

    #[test]
    fn cpu_adds_user_and_system_and_survives_a_name_with_spaces() {
        let stat = "42 (my node) S 1 42 42 0 -1 4194304 500 0 0 0 70 30 0 0 20 0 8 0 100 0 0";
        assert_eq!(parse_cpu_ticks(stat), Some(100));
        assert_eq!(parse_cpu_ticks("garbage"), None);
    }

    #[test]
    fn network_sums_every_interface_but_loopback() {
        let dev = "Inter-|   Receive  |  Transmit\n face |bytes packets|bytes\n\
            lo: 999 1 0 0 0 0 0 0 999 1 0 0 0 0 0 0\n\
            eth0: 100 1 0 0 0 0 0 0 200 1 0 0 0 0 0 0\n\
            eth1: 10 1 0 0 0 0 0 0 20 1 0 0 0 0 0 0\n";
        assert_eq!(parse_net(dev), Some((110, 220)));
    }
}
