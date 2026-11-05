#!/usr/bin/env bash
# finish-analysis.sh: after the final runs, every table and check, in one go. Run it from the code folder.
set -u
[ -n "${1:-}" ] && { grep -E 'client_gave_up|client 9: done' "$1" | cut -c1-200 >analysis/results/kills/t5.txt; }
python3 analysis/phase1/kills_analyze.py >analysis/results/kills/analysis.txt 2>&1
python3 analysis/phase1/kills_tables.py
python3 analysis/phase1/put_tables.py
. "$HOME/.cargo/env" 2>/dev/null
cargo test >analysis/results/kills/cargo-test.txt 2>&1
grep -E 'FAILED|^test result' analysis/results/kills/cargo-test.txt | awk '{f+=$6; p+=$4} END{print "cargo test: passed", p, "failed", f}'
