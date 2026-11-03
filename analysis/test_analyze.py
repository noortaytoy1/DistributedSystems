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


def done(start_s, latency_ms, executor, provisional=False, no_responder=False, attempts=1,
         status="assigned", priority=300):
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
        "status": status,
        "priority": priority,
    }


def udp_counters(ts_us, udp_drops=None, **counts):
    fields = {"rx_datagrams": 0, "rx_bytes": 0, "rx_max_bytes": 900, "rx_bad": 0, "rx_foreign": 0,
              "rx_requests": 0, "rx_cancels": 0, "rx_gate_dropped": 0, "rx_queue_full": 0,
              "channels_opened": 0, "tx_datagrams": 0, "tx_bytes": 0, "tx_max_bytes": 900,
              "tx_requests": 0, "tx_cancels": 0, "tx_replies": 0, "tx_would_block": 0,
              "tx_encode_errors": 0, "tx_errors": 0}
    return {"ts_us": ts_us, "ev": "udp_counters", "udp_drops": udp_drops, **fields, **counts}


def synthetic_run():
    client = [{"ts_us": T0, "ev": "client_sent", "request": request(0), "attempt": 0}]
    client.append(done(1, 999, 1))  # inside the 2 s warm-up: dropped
    # Ten measured requests: 10..100 ms, split 6/3/1 between the nodes. The first was refused.
    for i, executor in enumerate([1, 1, 1, 1, 1, 1, 2, 2, 2, 3]):
        client.append(done(3 + i * 0.5, (i + 1) * 10, executor, no_responder=i == 0,
                           status="expired" if i == 0 else "assigned",
                           priority=0 if i == 0 else 330))
    client.append(done(9, 1, 3, provisional=True, status="provisional"))
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
    client += [
        udp_counters(T0 + 2 * S, tx_requests=30, tx_cancels=20, tx_datagrams=50, udp_drops=7),
        udp_counters(T0 + 8 * S, tx_requests=63, tx_cancels=42, tx_datagrams=105, tx_would_block=1,
                     udp_drops=9),
        # The last line, as a client stops, has no drop count: it must not end up as a zero.
        udp_counters(T0 + 9 * S, tx_requests=63, tx_cancels=42, tx_datagrams=105, tx_would_block=1),
        {"ts_us": T0, "ev": "udp_socket", "local_addr": "0.0.0.0:1", "rcvbuf_asked_bytes": 8388608,
         "rcvbuf_got_bytes": 16777216},
    ]
    node1 += [
        udp_counters(T0 + 2 * S, rx_requests=10, rx_cancels=8, tx_datagrams=5, udp_drops=100),
        udp_counters(T0 + 8 * S, rx_requests=40, rx_cancels=28, tx_datagrams=16, rx_gate_dropped=3,
                     rx_queue_full=2, rx_bad=1, udp_drops=105),
        {"ts_us": T0, "ev": "udp_socket", "local_addr": "10.0.0.11:8001", "rcvbuf_asked_bytes": 8388608,
         "rcvbuf_got_bytes": 425984},
    ]
    return {"client-1": client, "node-1": node1, "node-2": node2, "node-3": node3}


def write_run(directory, events, run):
    directory.mkdir(parents=True)
    (directory / "run.json").write_text(json.dumps({
        "experiment": "e9", "cell": "synthetic", "run": run, "warmup_s": 2, "measured_s": 10,
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

    def test_reports_expired(self):
        self.assertEqual(self.m["reports_expired"], 1)

    def test_priority_mean_covers_every_answer(self):
        self.assertAlmostEqual(self.m["priority_mean"], (0 + 9 * 330 + 300) / 11)

    def test_a_cell_with_expired_reports_gets_a_warning(self):
        def cell(mean):
            return {("e1", "30-bidding"): {"reports_expired": (mean, float("nan"), 1)}}

        warnings = analyze.expired_warnings(cell(2.0))
        self.assertEqual(len(warnings), 1)
        self.assertIn("e1/30-bidding", warnings[0])
        self.assertEqual(analyze.expired_warnings(cell(0.0)), [])
        # A run with no answers has nothing to warn about, and neither does an older log.
        self.assertEqual(analyze.expired_warnings(cell(float("nan"))), [])
        self.assertEqual(analyze.expired_warnings({("e1", "old"): {}}), [])

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

    def test_udp_traffic_per_answered_request_includes_the_cancels(self):
        # 11 answered. Differences: 33 calls and 22 cancels sent, 55 + 11 datagrams both ways.
        self.assertAlmostEqual(self.m["udp_requests_per_req"], 3.0)
        self.assertAlmostEqual(self.m["udp_cancels_per_req"], 2.0)
        self.assertAlmostEqual(self.m["udp_datagrams_per_req"], 6.0)

    def test_udp_losses_and_drops(self):
        self.assertAlmostEqual(self.m["udp_request_loss_share"], 1 - 30 / 33)
        self.assertAlmostEqual(self.m["udp_cancel_loss_share"], 1 - 20 / 22)
        self.assertEqual(self.m["udp_gate_dropped"], 3)
        self.assertEqual(self.m["udp_queue_full"], 2)
        self.assertEqual(self.m["udp_bad"], 1)
        self.assertEqual(self.m["udp_send_lost"], 1)
        self.assertEqual(self.m["udp_drops_nodes"], 5)
        self.assertEqual(self.m["udp_drops_client_pc_max"], 2)
        self.assertEqual(self.m["udp_rcvbuf_got_min_bytes"], 425984)

    def test_a_cell_short_of_udp_buffer_gets_a_warning(self):
        warnings = analyze.udp_warnings({("e1", "c"): {
            "udp_rcvbuf_got_min_bytes": (425984, float("nan"), 1),
            "udp_drops_nodes": (5.0, float("nan"), 1),
            "udp_drops_client_pc_max": (0.0, float("nan"), 1),
        }})
        self.assertEqual(len(warnings), 2)
        self.assertIn("rmem_max", warnings[0])
        self.assertIn("udp_drops_nodes", warnings[1])
        clean = {"udp_rcvbuf_got_min_bytes": (16777216, float("nan"), 1),
                 "udp_drops_nodes": (0.0, float("nan"), 1)}
        self.assertEqual(analyze.udp_warnings({("e1", "c"): clean}), [])
        # An older log has none of these.
        self.assertEqual(analyze.udp_warnings({("e1", "old"): {}}), [])

    def test_a_log_from_before_the_udp_path_still_reads(self):
        self.assertEqual(analyze.udp_metrics({"client-1": [], "node-1": []}, 0, 5), {})

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


if __name__ == "__main__":
    unittest.main()
