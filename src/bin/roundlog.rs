//! roundlog <file>: what a node's log file says it applied, one "client seq" per report on standard
//! output and the counts on standard error. The kill experiments use it to check that every answer
//! a client got is in every node's record (D261). It only reads the file.

use std::collections::{HashMap, HashSet};
use std::io::{BufReader, BufWriter, Read, Write};

use anyhow::Context;
use rescuemesh::proto::{Command, LogRecord, RequestId, RoundBatch, decode};

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).context("usage: roundlog <file>")?;
    let file = std::fs::File::open(&path).with_context(|| format!("cannot open {path}"))?;
    let mut file = BufReader::new(file);
    let mut saved: HashMap<(u64, u64), RoundBatch> = HashMap::new();
    let mut reports: Vec<RequestId> = Vec::new();
    let mut seen: HashSet<RequestId> = HashSet::new();
    let (mut batches, mut duplicates, mut resets, mut order, mut cut_short) =
        (0u64, 0u64, 0u64, 0xcbf2_9ce4_8422_2325u64, false);
    let mut length = [0u8; 4];
    while file.read_exact(&mut length).is_ok() {
        let mut bytes = vec![0u8; u32::from_be_bytes(length) as usize];
        if file.read_exact(&mut bytes).is_err() {
            cut_short = true;
            break;
        }
        let Ok(record) = decode::<LogRecord>(&bytes) else {
            cut_short = true;
            break;
        };
        match record {
            LogRecord::Batch(batch) => {
                saved.insert((batch.node, batch.round), batch);
            }
            LogRecord::Applied { node, round } => {
                let Some(batch) = saved.remove(&(node, round)) else {
                    continue;
                };
                batches += 1;
                for word in [node, round] {
                    order = (order ^ word).wrapping_mul(0x0000_0100_0000_01b3);
                }
                for command in &batch.commands {
                    if let Command::CommitResult(result) = command {
                        if seen.insert(result.request) {
                            reports.push(result.request);
                        } else {
                            duplicates += 1;
                        }
                    }
                }
            }
            LogRecord::Reset => {
                resets += 1;
                batches = 0;
                duplicates = 0;
                order = 0xcbf2_9ce4_8422_2325;
                saved.clear();
                reports.clear();
                seen.clear();
            }
        }
    }
    let mut out = BufWriter::new(std::io::stdout().lock());
    for report in &reports {
        writeln!(out, "{} {}", report.client, report.seq)?;
    }
    out.flush()?;
    eprintln!(
        "batches={batches} reports={} duplicates={duplicates} resets={resets} not_applied={} order={order:016x} cut_short={cut_short}",
        reports.len(),
        saved.len()
    );
    Ok(())
}
