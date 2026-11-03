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
        {"ts_us": T0 + 4 * S, "ev": "state_checkpoint", "log_index": 100, "digest": 1111},
        {"ts_us": T0 + 5 * S, "ev": "silenced", "ms": 2000},
        {"ts_us": T0 + 7 * S, "ev": "restored"},
        {"ts_us": T0 + 7 * S, "ev": "rejoin_target_lowered", "from": 9, "to": 5},
        {"ts_us": T0 + 7 * S, "ev": "rejoined", "leader_commit": 5, "applied": 5, "recovery_us": 40_000},
        {"ts_us": T0 + 8 * S, "ev": "state_checkpoint", "log_index": 200, "digest": 2222},
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
        {"ts_us": T0 + 4 * S, "ev": "state_checkpoint", "log_index": 100, "digest": 1111},
        {"ts_us": T0 + 6 * S, "ev": "state_checkpoint", "log_index": 200, "digest": 2222},
        {"ts_us": T0 + 9 * S, "ev": "state_checkpoint", "log_index": 300, "digest": 3333},
    ]
    node3 = [
        {"ts_us": T0 + 1 * S, "ev": "overloaded", "request": request(0), "attempt": 0, "queued": 64, "running": 4},
        {"ts_us": T0 + 4 * S, "ev": "overloaded", "request": request(2), "attempt": 0, "queued": 64, "running": 4},
        {"ts_us": T0 + 5 * S, "ev": "overloaded", "request": request(3), "attempt": 1, "queued": 64, "running": 4},
        {"ts_us": T0 + 3 * S, "ev": "bid_round_closed", "request": request(1), "attempt": 0, "winner": 3,
         "i_won": True, "bids_seen": 2, "my_score": 1, "window_us": 40_000},
        {"ts_us": T0 + 4 * S, "ev": "bid_round_closed", "request": request(2), "attempt": 0, "winner": 2,
         "i_won": False, "bids_seen": 3, "my_score": 9, "window_us": 30_000},
        {"ts_us": T0 + 8 * S, "ev": "provisional_conflict", "request": request(7), "reason": "responder_taken",
         "responder": 1, "winner_responder": None, "winner_executor": None},
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

    def test_bids_and_split_decisions(self):
        self.assertEqual(self.m["bid_rounds"], 2)
        self.assertEqual(self.m["bid_split_decisions"], 1)
        self.assertAlmostEqual(self.m["bid_window_mean_ms"], 30.0)

    def test_elections_and_failover(self):
        self.assertEqual(self.m["elections"], 2)  # terms 2, 3, 4
        self.assertAlmostEqual(self.m["election_duration_mean_ms"], 200.0)
        self.assertEqual(self.m["detection_mean_ms"], 150)
        self.assertEqual(self.m["recovery_mean_ms"], 40.0)

    def test_a_term_running_before_the_window_is_not_an_election_and_a_new_one_is(self):
        # Term 2 started before the window and logged nothing inside it, so the window sees only
        # terms 3 and 4. Taking one off what it saw would say 1.
        events = synthetic_run()
        before_the_window = {"ts_us": T0, "ev": "raft_state", "term": 2, "leader": 1, "role": "leader", "last_applied": 1}
        events["node-1"] = [before_the_window] + [
            e for e in events["node-1"] if not (e["ev"] == "raft_state" and e["term"] == 2)
        ]
        events["node-2"] = [e for e in events["node-2"] if not (e["ev"] == "raft_state" and e["term"] == 2)]
        metrics = analyze.election_metrics(events, T0 + 2 * S)
        self.assertEqual(metrics["elections"], 2)

    def test_the_running_term_logged_again_inside_the_window_is_not_counted_twice(self):
        events = synthetic_run()
        before_the_window = {"ts_us": T0, "ev": "raft_state", "term": 2, "leader": 1, "role": "leader", "last_applied": 1}
        events["node-1"] = [before_the_window] + events["node-1"]
        metrics = analyze.election_metrics(events, T0 + 2 * S)
        self.assertEqual(metrics["elections"], 2)  # 3 and 4; the one running is 2

    def test_no_new_term_means_no_election(self):
        events = {"node-1": [
            {"ts_us": T0, "ev": "raft_state", "term": 5, "leader": 1, "role": "leader", "last_applied": 1},
        ]}
        self.assertEqual(analyze.election_metrics(events, T0 + 2 * S)["elections"], 0)
        self.assertEqual(analyze.election_metrics({"node-1": []}, T0)["elections"], 0)

    def test_takeovers_are_split_by_reason_and_none_is_called_failover(self):
        events = synthetic_run()
        takeover = {"ev": "takeover_started", "request": request(1), "attempt": 0, "winner": 1, "rank": 1}
        events["node-2"] += [
            {**takeover, "ts_us": T0 + 5 * S, "reason": "suspected", "waited_us": 150_000},
            {**takeover, "ts_us": T0 + 6 * S, "reason": "backstop", "waited_us": 200_000},
            {**takeover, "ts_us": T0 + 6 * S, "reason": "backstop", "waited_us": 400_000},
        ]
        metrics = analyze.fault_metrics(events, [])
        self.assertEqual(metrics["takeovers"], 3)
        self.assertEqual(metrics["takeovers_suspected"], 1)
        self.assertEqual(metrics["takeovers_backstop"], 2)
        self.assertAlmostEqual(metrics["takeover_wait_suspected_mean_ms"], 150.0)
        self.assertAlmostEqual(metrics["takeover_wait_backstop_mean_ms"], 300.0)
        self.assertNotIn("takeover_wait_mean_ms", metrics)

    def test_the_client_gap_is_the_longest_silence_that_overlaps_the_fault(self):
        # Answers at 3.01 ... 7.6 s every half second or so; the fault runs from 5 s to 7 s.
        def answer(at_s):
            return {"ts_us": T0 + int(at_s * S), "ev": "client_done"}

        events = {
            "client-1": [answer(4), answer(4.5), answer(8), answer(8.1)],
            "client-2": [answer(4.9), answer(6), answer(9), answer(20)],
        }
        metrics = analyze.client_gap_metrics(events, T0 + 5 * S, T0 + 7 * S)
        # Client 1: 3.5 s across the fault. Client 2: 3 s; its 11 s gap starts after the fault ended.
        self.assertAlmostEqual(metrics["client_gap_max_ms"], 3500.0)
        self.assertAlmostEqual(metrics["client_gap_mean_ms"], 3250.0)

    def test_a_client_that_never_hears_again_counts_its_whole_silence(self):
        def event(name, at_s):
            return {"ts_us": T0 + int(at_s * S), "ev": name}

        # Fault from 5 s to 7 s. The last answer is at 4 s, and the client tries until 30 s.
        events = {"client-1": [event("client_done", 3), event("client_done", 4), event("client_sent", 6),
                               event("client_gave_up", 30)]}
        metrics = analyze.client_gap_metrics(events, T0 + 5 * S, T0 + 7 * S)
        self.assertAlmostEqual(metrics["client_gap_max_ms"], 26000.0)

    def test_a_client_that_never_heard_at_all_counts_the_whole_run(self):
        events = {"client-1": [{"ts_us": T0 + 1 * S, "ev": "client_sent"}, {"ts_us": T0 + 9 * S, "ev": "client_gave_up"}]}
        metrics = analyze.client_gap_metrics(events, T0 + 5 * S, T0 + 7 * S)
        self.assertAlmostEqual(metrics["client_gap_max_ms"], 8000.0)

    def test_the_ends_of_a_normal_run_add_no_gap(self):
        def answer(at_s):
            return {"ts_us": T0 + int(at_s * S), "ev": "client_done"}

        # The waits before the first answer and after the last do not overlap this fault.
        events = {"client-1": [{"ts_us": T0, "ev": "client_sent"}, answer(4), answer(5.5), answer(7.5),
                               {"ts_us": T0 + 20 * S, "ev": "client_sent"}]}
        metrics = analyze.client_gap_metrics(events, T0 + 5 * S, T0 + 7 * S)
        self.assertAlmostEqual(metrics["client_gap_max_ms"], 2000.0)

    def test_a_run_with_a_fault_reports_the_client_gap(self):
        self.assertGreater(self.m["client_gap_max_ms"], 0)

    def test_an_election_right_after_a_rejoin_counts_against_it(self):
        self.assertEqual(self.m["elections_after_rejoin"], 1)  # term 4, 100 ms after

    def test_refused_planning_jobs_count_in_the_measured_time_per_node(self):
        self.assertEqual(self.m["plans_refused"], 2)  # the third is in the warm-up
        self.assertEqual(self.m["plans_refused_node_3"], 2)
        self.assertEqual(self.m["plans_refused_node_1"], 0)

    def test_digests_are_compared_at_the_same_index(self):
        self.assertEqual(self.m["digest_checkpoints_compared"], 2)  # 300 has one node's word only
        self.assertEqual(self.m["digest_mismatches"], 0)
        self.assertEqual(self.m["rejoins_digest_confirmed"], 1)
        self.assertEqual(self.m["rejoin_targets_lowered"], 1)

    def test_a_different_digest_at_an_index_is_a_mismatch_and_confirms_no_rejoin(self):
        events = synthetic_run()
        for e in events["node-1"]:
            if e["ev"] == "state_checkpoint" and e["log_index"] == 200:
                e["digest"] = 9999
        metrics = analyze.convergence_metrics(events)
        self.assertEqual(metrics["digest_mismatches"], 1)
        self.assertEqual(metrics["rejoins_digest_confirmed"], 0)

    def test_a_rejoin_with_no_checkpoint_after_it_is_not_confirmed(self):
        events = synthetic_run()
        events["node-1"] = [
            e for e in events["node-1"] if not (e["ev"] == "state_checkpoint" and e["log_index"] == 200)
        ]
        self.assertEqual(analyze.convergence_metrics(events)["rejoins_digest_confirmed"], 0)

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

    def test_a_run_the_runner_set_aside_is_not_read(self):
        write_run(self.root / "dropped" / "e9-synthetic-run-2", synthetic_run(), 2)
        self.assertEqual(analyze.summarise(self.root)[("e9", "synthetic")]["mean_ms"][2], 1)

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
