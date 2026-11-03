#!/usr/bin/env python3
"""Checks analyze.py against a small log written by hand. Every number here is synthetic: this file
tests the arithmetic and says nothing about RescueMesh.

    python analysis/test_analyze.py
"""

import json
import math
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import analyze  # noqa: E402

S = 1_000_000  # one second in microseconds
T0 = 1_000 * S


def request(seq):
    return {"client": 1, "seq": seq}


def done(start_s, latency_ms, executor, provisional=False, no_responder=False, attempts=1):
    latency_us = latency_ms * 1000
    return {
        "ts_us": T0 + int(start_s * S) + latency_us,
        "ev": "client_done",
        "request": request(int(start_s * 100)),
        "from": executor,
        "executor": executor,
        "latency_us": latency_us,
        "attempts": attempts,
        "provisional": provisional,
        "no_responder": no_responder,
    }


def synthetic_run():
    client = [{"ts_us": T0, "ev": "client_sent", "request": request(0), "attempt": 0}]
    client.append(done(1, 999, 1))  # inside the 2 s warm-up: dropped
    # Ten measured requests: 10..100 ms, split 6/3/1 between the nodes.
    for i, executor in enumerate([1, 1, 1, 1, 1, 1, 2, 2, 2, 3]):
        client.append(done(3 + i * 0.5, (i + 1) * 10, executor, no_responder=i == 0))
    client.append(done(9, 1, 3, provisional=True))
    client.append({"ts_us": T0 + 9 * S, "ev": "client_gave_up", "request": request(99), "attempts": 6})

    node1 = [
        {"ts_us": T0 + 3 * S, "ev": "raft_state", "term": 2, "leader": 1, "role": "leader", "last_applied": 1},
        {"ts_us": T0 + 5 * S, "ev": "silenced", "ms": 2000},
        {"ts_us": T0 + 7 * S, "ev": "restored"},
        {"ts_us": T0 + 7 * S, "ev": "rejoined", "leader_commit": 5, "applied": 5, "recovery_us": 40_000},
        {"ts_us": T0 + 3 * S, "ev": "resource_usage", "rss_kb": 2048, "cpu_ticks": 100,
         "net_rx_bytes": 0, "net_tx_bytes": 0, "queued": 2, "running": 1},
        {"ts_us": T0 + 4 * S, "ev": "resource_usage", "rss_kb": 4096, "cpu_ticks": 150,
         "net_rx_bytes": 500, "net_tx_bytes": 500, "queued": 4, "running": 1},
        {"ts_us": T0 + 3 * S, "ev": "messages", "sent": {"bid": 10}, "received": {}},
        {"ts_us": T0 + 8 * S, "ev": "messages", "sent": {"bid": 32}, "received": {}},
    ]
    node2 = [
        {"ts_us": T0 + 3 * S, "ev": "raft_state", "term": 2, "leader": 1, "role": "follower", "last_applied": 1},
        {"ts_us": T0 + 5 * S + 200_000, "ev": "peer_suspected", "peer": 1, "silent_ms": 150},
        {"ts_us": T0 + 5 * S + 300_000, "ev": "raft_state", "term": 3, "leader": None, "role": "candidate", "last_applied": 1},
        {"ts_us": T0 + 5 * S + 500_000, "ev": "raft_state", "term": 3, "leader": 2, "role": "leader", "last_applied": 1},
        {"ts_us": T0 + 3 * S, "ev": "bid_round_closed", "request": request(1), "attempt": 0, "winner": 2,
         "i_won": True, "bids_seen": 3, "my_score": 1, "window_us": 20_000},
        {"ts_us": T0 + 7 * S + 100_000, "ev": "raft_state", "term": 4, "leader": 2, "role": "leader", "last_applied": 1},
    ]
    node3 = [
        {"ts_us": T0 + 3 * S, "ev": "bid_round_closed", "request": request(1), "attempt": 0, "winner": 3,
         "i_won": True, "bids_seen": 2, "my_score": 1, "window_us": 40_000},
        {"ts_us": T0 + 4 * S, "ev": "bid_round_closed", "request": request(2), "attempt": 0, "winner": 2,
         "i_won": False, "bids_seen": 3, "my_score": 9, "window_us": 30_000},
        {"ts_us": T0 + 8 * S, "ev": "provisional_conflict", "request": request(7), "reason": "responder_taken",
         "responder": 1, "winner_responder": None, "winner_executor": None},
    ]
    return {"client-1": client, "node-1": node1, "node-2": node2, "node-3": node3}


def write_run(directory, events, run, **extra):
    directory.mkdir(parents=True)
    (directory / "run.json").write_text(json.dumps({
        "experiment": "e9", "cell": "synthetic", "run": run, "warmup_s": 2, "measured_s": 10, **extra,
    }))
    for source, lines in events.items():
        (directory / f"{source}.jsonl").write_text("\n".join(json.dumps(e) for e in lines) + "\n")


class Arithmetic(unittest.TestCase):
    def test_jain_is_one_when_even_and_one_over_n_when_not(self):
        self.assertAlmostEqual(analyze.jain([5, 5, 5]), 1.0)
        self.assertAlmostEqual(analyze.jain([9, 0, 0]), 1 / 3)
        self.assertTrue(math.isnan(analyze.jain([0, 0])))

    def test_percentiles_match_the_load_generator(self):
        values = list(range(1, 101))
        self.assertEqual(analyze.nearest_rank(values, 50), 50)
        self.assertEqual(analyze.nearest_rank(values, 99), 99)
        self.assertEqual(analyze.nearest_rank([7], 99), 7)

    def test_confidence_interval_uses_students_t(self):
        mean, half, n = analyze.confidence_interval([10.0, 12.0, 14.0])
        self.assertEqual((mean, n), (12.0, 3))
        # t(0.975, 2) = 4.3027, s = 2, sqrt(3): worked out by hand.
        self.assertAlmostEqual(half, 4.3027 * 2 / math.sqrt(3), places=3)

    def test_one_run_has_no_interval_and_gaps_are_skipped(self):
        mean, half, n = analyze.confidence_interval([5.0, float("nan")])
        self.assertEqual((mean, n), (5.0, 1))
        self.assertTrue(math.isnan(half))


class OneRun(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.run_dir = self.root / "e9" / "synthetic" / "run-1"
        write_run(self.run_dir, synthetic_run(), 1)
        _, self.m = analyze.analyse_run(self.run_dir)

    def tearDown(self):
        self.tmp.cleanup()

    def test_warm_up_is_dropped_and_provisional_answers_are_counted_apart(self):
        self.assertEqual(self.m["count"], 10)
        self.assertEqual(self.m["provisional_count"], 1)
        self.assertAlmostEqual(self.m["mean_ms"], 55.0)
        self.assertEqual(self.m["p50_ms"], 50.0)
        self.assertEqual(self.m["p99_ms"], 100.0)
        self.assertEqual(self.m["gave_up"], 1)

    def test_throughput_counts_every_answer_over_the_measured_time(self):
        self.assertAlmostEqual(self.m["throughput_rps"], 11 / 10)

    def test_load_shares_and_fairness(self):
        self.assertAlmostEqual(self.m["share_node_1"], 0.6)
        self.assertAlmostEqual(self.m["share_node_3"], 0.1)
        self.assertAlmostEqual(self.m["fairness_jain"], analyze.jain([6, 3, 1]))

    def test_no_responder_share(self):
        self.assertAlmostEqual(self.m["no_responder_share"], 1 / 11)

    def test_bids_and_split_decisions(self):
        self.assertEqual(self.m["bid_rounds"], 2)
        self.assertEqual(self.m["bid_split_decisions"], 1)
        self.assertAlmostEqual(self.m["bid_window_mean_ms"], 30.0)

    def test_elections_and_failover(self):
        self.assertEqual(self.m["elections"], 2)  # terms 2, 3, 4
        self.assertAlmostEqual(self.m["election_duration_mean_ms"], 200.0)
        self.assertEqual(self.m["detection_mean_ms"], 150)
        self.assertEqual(self.m["recovery_mean_ms"], 40.0)

    def test_an_election_right_after_a_rejoin_counts_against_it(self):
        self.assertEqual(self.m["elections_after_rejoin"], 1)  # term 4, 100 ms after

    def test_conflicts_by_reason(self):
        self.assertEqual(self.m["conflicts_responder_taken"], 1)
        self.assertEqual(self.m["conflicts_lost_first_commit"], 0)

    def test_phases_split_on_the_fault(self):
        # Requests start at 3, 3.5, ... 7.5 s; the fault runs from 5 s to 7 s.
        self.assertEqual(self.m["before_count"], 4)
        self.assertEqual(self.m["during_count"], 4)
        self.assertAlmostEqual(self.m["during_mean_ms"], 65.0)

    def test_messages_per_request_use_the_difference_in_counters(self):
        self.assertAlmostEqual(self.m["msgs_bid_per_req"], 22 / 11)

    def test_resources(self):
        self.assertAlmostEqual(self.m["cpu_cores_mean"], 0.5)
        self.assertAlmostEqual(self.m["rss_mb_max"], 4.0)
        self.assertAlmostEqual(self.m["net_bytes_per_s_mean"], 1000.0)
        self.assertEqual(self.m["queued_max"], 4)

    def test_a_whole_tree_gives_a_csv_and_an_interval_across_runs(self):
        slower = synthetic_run()
        for e in slower["client-1"]:
            if e["ev"] == "client_done":
                e["latency_us"] *= 2
                e["ts_us"] += e["latency_us"] // 2
        write_run(self.root / "e9" / "synthetic" / "run-2", slower, 2)
        summary = analyze.summarise(self.root)
        mean, half, n = summary[("e9", "synthetic")]["mean_ms"]
        self.assertEqual(n, 2)
        self.assertAlmostEqual(mean, 82.5)
        self.assertGreater(half, 0)
        out = self.root / "summary.csv"
        analyze.write_csv(summary, out)
        self.assertIn("e9,synthetic,mean_ms,2,82.5", out.read_text())


def sent(at_us, attempt=0):
    return {"ts_us": T0 + at_us, "ev": "client_sent", "request": request(0), "attempt": attempt}


def machine(at_s, busy, total, available_kb):
    return {"ts_us": T0 + at_s * S, "ev": "machine_usage", "cpu_busy_ticks": busy,
            "cpu_total_ticks": total, "mem_available_kb": available_kb}


class ClientPrograms(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def analyse(self, name, events, **extra):
        directory = self.root / "e9" / name / "run-1"
        write_run(directory, events, 1, **extra)
        return analyze.analyse_run(directory)[1]

    def test_three_client_logs_count_as_three_programs(self):
        events = {f"client-{i}": [sent(0)] for i in (10000, 10001, 20000)}
        # A program that never sent anything did not take part.
        events["client-30000"] = [{"ts_us": T0, "ev": "client_link_up", "node": 1}]
        m = self.analyse("three", events, users=30)
        self.assertEqual(m["client_programs"], 3)
        self.assertAlmostEqual(m["users_per_program"], 10.0)

    def test_a_run_missing_programs_is_left_out_of_the_summary(self):
        events = synthetic_run()
        write_run(self.root / "e9" / "whole" / "run-1", events, 1, client_programs_expected=1)
        write_run(self.root / "e9" / "whole" / "run-2", events, 2, client_programs_expected=2)
        summary = analyze.summarise(self.root)
        self.assertEqual(summary[("e9", "synthetic")]["mean_ms"][2], 1)
        _, m = analyze.analyse_run(self.root / "e9" / "whole" / "run-2")
        self.assertIn("1 of 2", analyze.run_problem({"client_programs_expected": 2}, m))

    def test_a_victim_with_no_silenced_event_is_left_out(self):
        quiet = {"client-1": [sent(0)], "node-1": []}
        m = self.analyse("quiet", quiet)
        self.assertIn("no node logged a silence", analyze.run_problem({"victim": "leader"}, m))
        self.assertIsNone(analyze.run_problem({"victim": "none"}, m))
        loud = {"client-1": [sent(0)], "node-1": [{"ts_us": T0, "ev": "silenced", "ms": 1}]}
        self.assertIsNone(analyze.run_problem({"victim": "leader"}, self.analyse("loud", loud)))

    def test_a_run_json_without_the_new_keys_skips_the_checks(self):
        m = self.analyse("old", {"client-1": [], "node-1": []})
        self.assertIsNone(analyze.run_problem({}, m))

    def test_link_drops_are_counted_by_reason(self):
        down = [{"ts_us": T0, "ev": "client_link_down", "node": 1, "reason": reason}
                for reason in ("closed by the node", "no answer to pings", "closed by the node")]
        m = self.analyse("links", {"client-1": [sent(0)] + down})
        self.assertEqual(m["client_link_down"], 3)
        self.assertEqual(m["client_link_down_closed_by_the_node"], 2)
        self.assertEqual(m["client_link_down_no_answer_to_pings"], 1)
        self.assertEqual(self.analyse("no-links", {"client-1": [sent(0)]})["client_link_down"], 0)

    def test_arrival_peak_share_is_a_tenth_when_even_and_one_when_all_together(self):
        even = [i * 10_000 for i in range(1000)]  # one every 10 ms for ten seconds
        self.assertAlmostEqual(analyze.arrival_peak_share(even), 0.1)
        together = [5 * S + i for i in range(100)]
        self.assertEqual(analyze.arrival_peak_share(together), 1.0)
        self.assertTrue(math.isnan(analyze.arrival_peak_share([])))

    def test_arrivals_are_read_at_the_start_in_the_measured_time_and_after_a_fault(self):
        # Warm-up 2 s. A first wave of 100 together, then even arrivals, and retries not counted.
        client = [sent(i) for i in range(100)]
        client += [sent(1 * S + i * 10_000) for i in range(600)]
        client += [sent(2 * S, attempt=1) for _ in range(500)]
        # After the fault, at 7 s, everyone comes back within one 100 ms.
        client += [sent(7 * S + 500_000 + i) for i in range(100)]
        node = [{"ts_us": T0 + 7 * S, "ev": "restored"}]
        m = self.analyse("arrivals", {"client-1": client, "node-1": node})
        self.assertEqual(m["arrival_peak_100ms_share_first_s"], 1.0)
        self.assertGreater(m["arrival_peak_100ms_share"], 0.1)
        self.assertEqual(m["arrival_peak_100ms_share_after_fault"], 1.0)

    def test_machine_usage_gives_cpu_p95_and_memory_min(self):
        # Busy shares of 10, 20 and 50 percent, then a sample with nothing readable.
        samples = [machine(3, 100, 1000, 9000), machine(4, 110, 1100, 8000),
                   machine(5, 130, 1200, 4096), machine(6, 180, 1300, 7000),
                   machine(7, None, None, None)]
        m = self.analyse("usage", {"client-10000": [sent(0)] + samples})
        self.assertAlmostEqual(m["client_pc_cpu_busy_p95"], 0.5)
        self.assertAlmostEqual(m["client_pc_mem_available_min_mb"], 4.0)
        # Samples from the warm-up do not count.
        early = [machine(0, 0, 0, 1)]
        m = self.analyse("early", {"client-10000": [sent(0)] + early + samples})
        self.assertAlmostEqual(m["client_pc_mem_available_min_mb"], 4.0)
        # A run with no samples has no such metric: nothing pretends the PC was idle.
        self.assertNotIn("client_pc_cpu_busy_p95", self.analyse("none", {"client-1": [sent(0)]}))

    def whole_run(self, name, run, script_output=None, **extra):
        """A good run, with what one machine's start-users.sh printed, if anything."""
        directory = self.root / "e9" / name / f"run-{run}"
        write_run(directory, synthetic_run(), run, **extra)
        if script_output is not None:
            (directory / "users-1-0.out").write_text(script_output)
        return directory

    def test_a_program_that_died_half_way_is_left_out(self):
        # It sent requests, so it counts among the programs that took part. Only the script knows.
        self.whole_run("whole", 1, "machine 1 batch 0: started 1, failed 0\n", client_programs_expected=1)
        died = self.whole_run("whole", 2, "machine 1 batch 0: client 10000 exited with status 137\n"
                              "machine 1 batch 0: started 1, failed 1\n", client_programs_expected=1)
        summary = analyze.summarise(self.root)
        self.assertEqual(summary[("e9", "synthetic")]["mean_ms"][2], 1)
        meta, m = analyze.analyse_run(died)
        self.assertIn("1 client programs failed", analyze.run_problem(meta, m))

    def test_programs_a_signal_stopped_early_are_left_out(self):
        cut = self.whole_run("cut", 1, "stopped by a signal: requests still in flight are not counted\n"
                             "machine 1 batch 0: stopped early\nmachine 1 batch 0: started 1, failed 0\n")
        meta, m = analyze.analyse_run(cut)
        self.assertIn("stopped 1 client programs early", analyze.run_problem(meta, m))

    def test_a_script_that_never_printed_totals_is_left_out(self):
        killed = self.whole_run("killed", 1, "plan machine=1 batch=0 users=1 programs=1\n")
        meta, m = analyze.analyse_run(killed)
        self.assertIn("never printed their totals", analyze.run_problem(meta, m))

    def test_half_a_last_line_is_dropped_and_the_run_still_reads(self):
        directory = self.whole_run("torn", 1)
        whole, _ = analyze.analyse_run(directory)
        log = directory / "client-1.jsonl"
        log.write_text(log.read_text() + '{"ts_us":17000,"src":"client-1","ev":"client_do')
        again, m = analyze.analyse_run(directory)
        self.assertEqual(whole, again)
        self.assertIsNone(analyze.run_problem(again, m))

    def test_a_damaged_log_leaves_only_its_own_run_out(self):
        self.whole_run("whole", 1)
        damaged = self.whole_run("whole", 2)
        log = damaged / "client-1.jsonl"
        lines = log.read_text().split("\n")
        lines[0] = lines[0][:20]
        log.write_text("\n".join(lines))
        summary = analyze.summarise(self.root)
        self.assertEqual(summary[("e9", "synthetic")]["mean_ms"][2], 1)

    def test_a_link_drop_reason_counts_as_zero_in_a_run_that_did_not_have_it(self):
        down = {"ts_us": T0, "ev": "client_link_down", "node": 1, "reason": "send failed"}
        quiet = synthetic_run()
        loud = synthetic_run()
        loud["client-1"] = loud["client-1"] + [down, down]
        write_run(self.root / "e9" / "s" / "run-1", quiet, 1)
        write_run(self.root / "e9" / "s" / "run-2", loud, 2)
        mean, _, runs = analyze.summarise(self.root)[("e9", "synthetic")]["client_link_down_send_failed"]
        self.assertEqual((mean, runs), (1.0, 2))


if __name__ == "__main__":
    unittest.main()
