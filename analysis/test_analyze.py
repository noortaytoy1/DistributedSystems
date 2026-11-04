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


def election(at_s, round_number, victim, ms, called, election_us):
    return {
        "ts_us": T0 + at_s * S,
        "ev": "fault_election",
        "round": round_number,
        "tickets": {"1": 10, "2": 20},
        "victim": victim,
        "ms": ms,
        "called": called,
        "election_us": election_us,
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
        {"ts_us": T0 + 3 * S, "ev": "messages", "sent": {"bid": 10, "fault_bid": 0}, "received": {}},
        {"ts_us": T0 + 8 * S, "ev": "messages", "sent": {"bid": 32, "fault_bid": 24}, "received": {}},
        election(1, 0, 3, 5000, True, 99_000),  # inside the 2 s warm-up: dropped
        election(3, 1, 2, 5000, True, 3000),
        election(4, 2, None, None, True, None),
        election(5, 3, 3, 4000, True, 5000),
        election(6, 4, 1, 6000, False, 100),
        {"ts_us": T0 + 6 * S, "ev": "fault_refused", "victim": 1, "reason": "recovering: not serving yet"},
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
        election(3, 1, 2, 5000, False, 500),
        election(4, 2, None, None, False, None),
        election(5, 3, 3, 4000, False, 200),
        election(6, 4, 2, 7000, True, 7000),  # names another node than node 1 does: a split
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
        election(3, 1, 2, 5000, False, 700),
        election(5, 3, None, None, False, None),  # missed a bid, so it never knew it had won
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


def raft_state(at_s, leader):
    return {"ts_us": T0 + at_s * S, "ev": "raft_state", "term": 2, "leader": leader, "role": "follower",
            "last_applied": 1}


def commit(at_s, latency_us, forwarded, kind="commit_result"):
    return {"ts_us": T0 + at_s * S, "ev": "commit", "kind": kind, "outcome": "committed",
            "latency_us": latency_us, "forwarded": forwarded, "log_index": 1, "request": request(1)}


class TheLeadersRole(unittest.TestCase):
    """Measuring starts at T0 + 10 s in these cases."""

    def share(self, node1, node2):
        events = {"node-1": node1, "node-2": node2, "client-1": []}
        return analyze.leader_plan_share(events, T0 + 10 * S, {"share_node_1": 0.2, "share_node_2": 0.5})

    def test_it_is_the_share_of_the_node_that_led_throughout(self):
        self.assertEqual(self.share([raft_state(1, 2)], [raft_state(1, 2)]), 0.5)

    def test_a_change_before_measuring_starts_does_not_count(self):
        self.assertEqual(self.share([raft_state(1, 1), raft_state(2, 2)], [raft_state(3, 2)]), 0.5)

    def test_a_change_inside_the_measured_time_gives_nan(self):
        self.assertTrue(math.isnan(self.share([raft_state(1, 1), raft_state(11, 2)], [raft_state(1, 1)])))

    def test_a_moment_with_no_leader_gives_nan(self):
        self.assertTrue(math.isnan(self.share([raft_state(1, 2), raft_state(11, None)], [raft_state(1, 2)])))
        # Even if the same node leads again afterwards: it did not lead throughout.
        node = [raft_state(1, 2), raft_state(11, None), raft_state(12, 2)]
        self.assertTrue(math.isnan(self.share(node, [raft_state(1, 2)])))

    def test_two_nodes_naming_different_leaders_gives_nan(self):
        # A silenced leader keeps reading itself while the others move on.
        self.assertTrue(math.isnan(self.share([raft_state(1, 1)], [raft_state(1, 2)])))

    def test_no_raft_state_at_all_gives_nan(self):
        self.assertTrue(math.isnan(self.share([], [])))

    def test_commit_latency_splits_on_whether_the_proposal_was_forwarded(self):
        events = {
            "node-1": [commit(11, 1000, False), commit(11, 3000, False), commit(1, 9000, False)],
            "node-2": [commit(12, 8000, True), commit(12, 5000, True, kind="release_responder")],
        }
        m = analyze.commit_metrics(events, T0 + 10 * S)
        self.assertEqual(m["commit_count"], 3)
        self.assertEqual(m["commit_local_count"], 2)
        self.assertAlmostEqual(m["commit_local_mean_ms"], 2.0)
        self.assertEqual(m["commit_forwarded_count"], 1)
        self.assertAlmostEqual(m["commit_forwarded_mean_ms"], 8.0)

    def test_a_run_with_one_path_has_nothing_on_the_other(self):
        m = analyze.commit_metrics({"node-1": [commit(11, 1000, False)]}, T0 + 10 * S)
        self.assertEqual(m["commit_forwarded_count"], 0)
        self.assertTrue(math.isnan(m["commit_forwarded_mean_ms"]))

    def test_a_whole_run_reports_both(self):
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = Path(tmp) / "run"
            write_run(run_dir, synthetic_run(), 1)
            _, m = analyze.analyse_run(run_dir)
        # The leader changes in this run.
        self.assertTrue(math.isnan(m["leader_plan_share"]))
        self.assertEqual(m["commit_local_count"], 0)


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

    def test_fault_elections(self):
        self.assertEqual(self.m["fault_elections"], 3)  # rounds 1, 3 and 4; round 0 is warm-up
        self.assertEqual(self.m["fault_rounds_abandoned"], 1)  # round 2
        # Called lines only, nulls skipped: 3000, 5000 and 7000 us.
        self.assertAlmostEqual(self.m["fault_election_mean_ms"], 5.0)
        self.assertEqual(self.m["fault_split_decisions"], 1)  # round 4
        self.assertEqual(self.m["fault_elected_not_silenced"], 1)  # round 3
        self.assertEqual(self.m["fault_refused"], 1)
        self.assertAlmostEqual(self.m["fault_bids_per_round"], 24 / 4)
        self.assertAlmostEqual(self.m["msgs_fault_bid_per_req"], 24 / 11)

    def test_a_run_with_no_fault_election_has_counts_of_zero_and_no_mean(self):
        m = analyze.fault_election_metrics({"node-1": [], "client-1": []}, 0)
        self.assertEqual(m["fault_elections"], 0)
        self.assertEqual(m["fault_split_decisions"], 0)
        self.assertTrue(math.isnan(m["fault_election_mean_ms"]))
        self.assertTrue(math.isnan(m["fault_bids_per_round"]))

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

    def test_udp_traffic_per_answered_request_includes_the_cancels(self):
        # 11 answered. Differences: 33 calls and 22 cancels sent, 55 + 11 datagrams both ways.
        self.assertAlmostEqual(self.m["udp_requests_per_req"], 3.0)
        self.assertAlmostEqual(self.m["udp_cancels_per_req"], 2.0)
        self.assertAlmostEqual(self.m["udp_datagrams_per_req"], 6.0)

    def test_udp_losses_and_drops(self):
        # Over the whole log, so the last counts: 63 calls and 42 cancels sent, 40 and 28 read.
        self.assertAlmostEqual(self.m["udp_request_loss_share"], 1 - 40 / 63)
        self.assertAlmostEqual(self.m["udp_cancel_loss_share"], 1 - 28 / 42)
        self.assertEqual(self.m["udp_gate_dropped"], 3)
        self.assertEqual(self.m["udp_queue_full"], 2)
        self.assertEqual(self.m["udp_bad"], 1)
        self.assertEqual(self.m["udp_send_lost"], 1)
        self.assertEqual(self.m["udp_drops_nodes"], 5)
        self.assertEqual(self.m["udp_drops_client_pc_max"], 2)
        self.assertEqual(self.m["udp_rcvbuf_got_min_bytes"], 425984)

    def test_nothing_lost_reads_as_nothing_lost_whenever_each_process_samples(self):
        # The node's first sample in the measured time comes most of a second after the client's. Measured
        # from there, the client seems to have sent 2700 and the node to have read 2430: 10 percent "lost".
        client = [udp_counters(T0 + S, tx_requests=300, tx_cancels=200),
                  udp_counters(T0 + 10 * S, tx_requests=3000, tx_cancels=2000)]
        node = [udp_counters(T0 + S + S // 10 * 9, rx_requests=570, rx_cancels=380),
                udp_counters(T0 + 11 * S, rx_requests=3000, rx_cancels=2000)]
        m = analyze.udp_metrics({"client-1": client, "node-1": node}, T0 + S // 2, 100)
        self.assertEqual(m["udp_request_loss_share"], 0.0)
        self.assertEqual(m["udp_cancel_loss_share"], 0.0)

    def test_a_restarted_node_counts_every_life_in_the_udp_loss_share(self):
        # Its counters start again from 0 after the restart, in the same log.
        client = [udp_counters(T0, tx_requests=100), udp_counters(T0 + 9 * S, tx_requests=1000)]
        node = [udp_counters(T0, rx_requests=100), udp_counters(T0 + 3 * S, rx_requests=400),
                udp_counters(T0 + 6 * S, rx_requests=100), udp_counters(T0 + 9 * S, rx_requests=500)]
        m = analyze.udp_metrics({"client-1": client, "node-1": node}, T0, 100)
        self.assertAlmostEqual(m["udp_request_loss_share"], 1 - 900 / 1000)

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

    def test_warm_up_counts_from_the_programs_start_and_not_its_first_send(self):
        # A random pattern's only user waited 1.5 s before its first send. Warm-up is 2 s, on the program's
        # clock, so a request sent 2.5 s in is measured. Counting from the first send would drop it.
        started = {"ts_us": T0, "ev": "client_started", "users": 1}
        client = [started, sent(int(1.5 * S)), done(1.5, 10, 1), sent(int(2.5 * S)), done(2.5, 10, 1)]
        self.assertEqual(self.analyse("late", {"client-10000": client})["count"], 1)
        # Without the event, the first send is all there is.
        self.assertEqual(self.analyse("old", {"client-10000": client[1:]})["count"], 0)

    def test_a_request_started_in_the_warm_up_does_not_count_when_it_gives_up_later(self):
        def sent_as(seq, at_s):
            return {"ts_us": T0 + at_s * S, "ev": "client_sent", "request": request(seq), "attempt": 0}

        def gave_up(seq, at_s):
            return {"ts_us": T0 + at_s * S, "ev": "client_gave_up", "request": request(seq), "attempts": 6}

        # Warm-up is 2 s. Request 1 starts in it, and request 2 after it. Both give up after it.
        client = [sent_as(1, 0), sent_as(2, 3), gave_up(1, 30), gave_up(2, 33)]
        self.assertEqual(self.analyse("gave-up", {"client-10000": client})["gave_up"], 1)

    def test_users_that_do_not_add_up_to_the_cell_are_left_out(self):
        program = {"ts_us": T0, "ev": "client_started", "users": 33}
        m = self.analyse("users", {"client-10000": [program, sent(0)], "client-20000": [program, sent(0)]})
        self.assertEqual(m["client_users"], 66)
        self.assertIn("66 users ran", analyze.run_problem({"users": 100}, m))
        self.assertIsNone(analyze.run_problem({"users": 66}, m))

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

    def test_a_node_log_that_was_not_copied_is_left_out(self):
        # A machine lost at the end of a run: without its log, fairness would read as two nodes' share.
        whole = self.whole_run("nodes", 1, nodes=3)
        meta, m = analyze.analyse_run(whole)
        self.assertIsNone(analyze.run_problem(meta, m))
        (whole / "node-2.jsonl").unlink()
        meta, m = analyze.analyse_run(whole)
        self.assertIn("2 of 3 node logs", analyze.run_problem(meta, m))

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


def state(at_ms, term, role, leader=None):
    return {"ts_us": T0 + at_ms * 1000, "ev": "raft_state", "term": term, "role": role, "leader": leader,
            "last_applied": 1}


def committed(at_ms, outcome="committed", kind="commit_result"):
    return {"ts_us": T0 + at_ms * 1000, "ev": "commit", "kind": kind, "outcome": outcome, "latency_us": 500,
            "forwarded": False, "log_index": 1, "request": request(1)}


def silence(at_ms, ms=5000):
    return {"ts_us": T0 + at_ms * 1000, "ev": "silenced", "ms": ms}


def restored(at_ms):
    return {"ts_us": T0 + at_ms * 1000, "ev": "restored"}


def aside(at_ms, term):
    return {"ts_us": T0 + at_ms * 1000, "ev": "leader_stepped_aside", "term": term}


def aside_ended(at_ms, term, outcome, new_term=None):
    return {"ts_us": T0 + at_ms * 1000, "ev": "leader_step_aside_ended", "term": term, "outcome": outcome,
            "new_term": term if new_term is None else new_term, "new_leader": 1, "waited_ms": 3000}


class Failover(unittest.TestCase):
    """Measuring starts at T0 in these cases, and times are in ms after it."""

    def failover(self, node1, node2=(), measured_s=None):
        logs = {"node-1": list(node1), "node-2": list(node2), "client-1": []}
        return analyze.failover_metrics(logs, T0, measured_s)

    def test_it_runs_from_the_silence_to_the_new_leader_and_its_first_commit(self):
        m = self.failover(
            [state(0, 2, "leader", 1), silence(1000)],
            [committed(1000), state(0, 2, "follower", 1), state(1735, 3, "candidate"),
             state(1745, 3, "leader", 2), committed(1751), committed(1800)],
        )
        self.assertEqual(m["failovers"], 1)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 745.0)
        self.assertAlmostEqual(m["failover_to_commit_ms_mean"], 751.0)

    def test_the_vote_figure_is_only_the_vote(self):
        # What the rehearsal saw: the old figure read 10 ms where nobody could commit for 745 ms.
        node1 = [state(0, 2, "leader", 1), silence(1000)]
        node2 = [state(0, 2, "follower", 1), state(1735, 3, "candidate"), state(1745, 3, "leader", 2)]
        events = {"node-1": node1, "node-2": node2}
        self.assertAlmostEqual(analyze.election_metrics(events, T0)["election_duration_mean_ms"], 10.0)
        self.assertAlmostEqual(analyze.failover_metrics(events, T0)["failover_to_leader_ms_mean"], 745.0)

    def test_a_silenced_follower_loses_no_leader(self):
        m = self.failover([state(0, 2, "follower", 2), silence(1000)], [state(0, 2, "leader", 2)])
        self.assertEqual(m["failovers"], 0)
        self.assertTrue(math.isnan(m["failover_to_leader_ms_mean"]))
        self.assertTrue(math.isnan(m["failover_to_commit_ms_mean"]))

    def test_a_step_aside_counts_from_when_the_leader_went_quiet(self):
        m = self.failover(
            [state(0, 4, "leader", 1), aside(2000, 4), aside_ended(2800, 4, "changed")],
            [state(2760, 5, "leader", 2), committed(2770)],
        )
        self.assertEqual(m["failovers"], 1)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 760.0)
        self.assertAlmostEqual(m["failover_to_commit_ms_mean"], 770.0)

    def test_a_step_aside_nobody_took_up_loses_no_leader(self):
        m = self.failover(
            [state(0, 4, "leader", 1), aside(2000, 4), aside_ended(5000, 4, "timed_out")],
            # Much later a real fault elects someone. That has nothing to do with the step-aside.
            [state(90000, 5, "leader", 2)],
        )
        self.assertEqual(m["failovers"], 0)
        self.assertEqual(m["failovers_without_leader"], 0)

    def test_an_end_line_with_no_start_line_does_not_stop_the_analysis(self):
        # A clock stepped back can sort the end first. One odd line must not cost the whole summary. The
        # commit keeps the loss away from the end of the logs, where it would not be judged.
        m = self.failover([state(0, 4, "leader", 1), aside_ended(5000, 4, "timed_out"), aside(5001, 4)],
                          [committed(9000)])
        self.assertEqual(m["failovers"], 1)
        self.assertEqual(m["failovers_without_leader"], 1)

    def test_a_later_silence_in_a_term_whose_step_aside_timed_out_still_counts(self):
        m = self.failover(
            [state(0, 4, "leader", 1), aside(2000, 4), aside_ended(5000, 4, "timed_out"), silence(70000)],
            [state(70700, 5, "leader", 2)],
        )
        self.assertEqual(m["failovers"], 1)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 700.0)

    def test_a_leader_that_goes_quiet_and_then_silent_counts_once(self):
        m = self.failover(
            [state(0, 4, "leader", 1), aside(2000, 4), silence(2100), aside_ended(2150, 4, "stopped")],
            [state(2760, 5, "leader", 2)],
        )
        self.assertEqual(m["failovers"], 1)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 760.0)

    def test_a_leader_that_wins_its_own_vote_again_counts(self):
        m = self.failover(
            [state(0, 4, "leader", 1), aside(2000, 4), state(2700, 5, "candidate"), state(2710, 5, "leader", 1),
             aside_ended(2720, 4, "re_elected")],
        )
        self.assertEqual(m["failovers"], 1)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 710.0)

    def test_a_leader_lost_with_nobody_to_follow_it_is_counted_apart(self):
        # It never came back, and the logs run on well past the silence.
        m = self.failover([state(0, 2, "leader", 1), silence(1000)], [state(0, 2, "follower", 1), committed(9000)])
        self.assertEqual(m["failovers"], 1)
        self.assertEqual(m["failovers_without_leader"], 1)
        self.assertTrue(math.isnan(m["failover_to_leader_ms_mean"]))

    def test_a_silence_the_leader_outlives_is_no_loss(self):
        # A real run's shape: 134 ms silent, the same term before and after, and nobody took over.
        m = self.failover([state(0, 2, "leader", 1), silence(6426, ms=134), restored(6561)],
                          [state(0, 2, "follower", 1), committed(20000)])
        self.assertEqual(m["failovers"], 0)
        self.assertEqual(m["failovers_without_leader"], 0)
        self.assertTrue(math.isnan(m["failover_to_leader_ms_mean"]))

    def test_two_silences_in_one_term_count_from_the_last(self):
        m = self.failover(
            [state(0, 2, "leader", 1), silence(1000, ms=134), restored(1134), silence(10000, ms=900),
             restored(10900), state(10950, 3, "follower", 2)],
            [state(0, 2, "follower", 1), state(10740, 3, "candidate"), state(10750, 3, "leader", 2), committed(10760)],
        )
        self.assertEqual(m["failovers"], 1)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 750.0)
        self.assertAlmostEqual(m["failover_to_commit_ms_mean"], 760.0)

    def test_a_short_silence_does_not_take_the_blame_for_a_later_election(self):
        # A real run's shape: 279 ms silent, and the term moved 28 s later for another reason.
        m = self.failover([state(0, 5, "leader", 1), silence(1000, ms=279), restored(1279)],
                          [state(29400, 6, "candidate"), state(29410, 6, "leader", 2), committed(29420)])
        self.assertEqual(m["failovers"], 0)
        self.assertTrue(math.isnan(m["failover_to_leader_ms_mean"]))

    def test_a_silence_that_cost_the_term_counts_even_if_the_new_leader_is_slow(self):
        # 600 ms silent, a vote refused while it lasted, and a new leader 1.4 s after it began.
        m = self.failover([state(0, 2, "leader", 1), silence(1000, ms=600), restored(1600)],
                          [state(1500, 3, "candidate"), state(2400, 4, "leader", 2)])
        self.assertEqual(m["failovers"], 1)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 1400.0)

    def test_with_no_majority_left_it_reads_the_outage(self):
        # A lone node's terms climb during the silence, and the new leader comes well after it ends.
        m = self.failover(
            [state(0, 2, "leader", 1), silence(1000, ms=20000), restored(21000), state(24100, 10, "follower", 2)],
            [state(1800, 3, "candidate"), state(9000, 7, "candidate"), state(24041, 10, "leader", 2), committed(24050)],
        )
        self.assertEqual(m["failovers"], 1)
        self.assertEqual(m["failovers_without_leader"], 0)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 23041.0)

    def test_a_step_aside_that_timed_out_after_the_term_moved_still_counts(self):
        # A vote was still running at the 3 s limit, so the line says timed_out with a higher new_term.
        m = self.failover(
            [state(0, 4, "leader", 1), aside(2000, 4), state(2750, 5, "follower"),
             aside_ended(5000, 4, "timed_out", new_term=5), state(5205, 5, "follower", 2)],
            [state(5200, 5, "leader", 2), committed(5210)],
        )
        self.assertEqual(m["failovers"], 1)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 3200.0)
        self.assertAlmostEqual(m["failover_to_commit_ms_mean"], 3210.0)

    def test_a_step_aside_called_off_does_not_take_the_blame_for_a_later_election(self):
        m = self.failover([state(0, 4, "leader", 1), aside(2000, 4), aside_ended(2100, 4, "stopped")],
                          [state(90000, 5, "leader", 2)])
        self.assertEqual(m["failovers"], 0)

    def test_a_loss_after_the_measured_time_does_not_count(self):
        m = self.failover([state(0, 2, "leader", 1), silence(61000)], [state(61700, 3, "leader", 2)], measured_s=60)
        self.assertEqual(m["failovers"], 0)

    def test_a_loss_at_the_end_of_the_measured_time_counts(self):
        m = self.failover([state(0, 2, "leader", 1), silence(59500)], [state(60200, 3, "leader", 2)], measured_s=60)
        self.assertEqual(m["failovers"], 1)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 700.0)

    def test_a_loss_too_near_the_end_of_the_logs_is_not_judged(self):
        # Silenced just before the nodes stopped: nobody had time to take over.
        m = self.failover([state(0, 2, "leader", 1), silence(59000), {"ts_us": T0 + 59_300_000, "ev": "stopped"}],
                          [state(0, 2, "follower", 1)], measured_s=60)
        self.assertEqual(m["failovers"], 0)
        self.assertEqual(m["failovers_without_leader"], 0)

    def test_a_new_leader_with_no_commit_after_it_is_counted_apart(self):
        m = self.failover(
            [state(0, 2, "leader", 1), silence(1000), state(9000, 3, "follower", 2), state(55000, 4, "leader", 1)],
            [state(1700, 3, "leader", 2), committed(1720), silence(50000)],
        )
        self.assertEqual(m["failovers"], 2)
        self.assertEqual(m["failovers_without_commit"], 1)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 2850.0)
        self.assertAlmostEqual(m["failover_to_commit_ms_mean"], 720.0)

    def test_a_clock_far_behind_does_not_pair_a_loss_with_a_later_election(self):
        # Node 2's clock is 900 ms behind, so it seems to lead before the silence. Node 3's election, 60 s
        # on, belongs to node 2's own loss.
        logs = {"node-1": [state(0, 2, "leader", 1), silence(1000)],
                "node-2": [state(800, 3, "leader", 2), committed(810), silence(60000)],
                "node-3": [state(60700, 4, "leader", 3), committed(60710)]}
        m = analyze.failover_metrics(logs, T0)
        self.assertEqual(m["failovers"], 2)
        self.assertEqual(m["failovers_without_leader"], 1)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 700.0)

    def test_a_lost_leader_with_no_next_one_gets_a_warning(self):
        cell = {("e3e4", "x"): {"failovers_without_leader": (0.33, 1.43, 3)}, ("e1", "y"): {}}
        warnings = analyze.failover_warnings(cell)
        self.assertEqual(len(warnings), 1)
        self.assertIn("e3e4/x", warnings[0])
        self.assertIn("failovers_without_leader is 0.33", warnings[0])
        self.assertEqual(analyze.failover_warnings({("e3e4", "x"): {"failovers_without_leader": (0.0, 0.0, 3)}}), [])

    def test_each_loss_in_a_run_counts(self):
        m = self.failover(
            [state(0, 2, "leader", 1), silence(1000), state(1900, 3, "follower", 2)],
            [state(1700, 3, "leader", 2), silence(5000), committed(1710), committed(8000)],
        )
        # Node 2 goes silent at 5 s with no later leader: 700 ms, and one without.
        self.assertEqual(m["failovers"], 2)
        self.assertEqual(m["failovers_without_leader"], 1)
        self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 700.0)
        self.assertAlmostEqual(m["failover_to_commit_ms_mean"], 710.0)

    def test_a_loss_in_the_warm_up_does_not_count(self):
        events = {"node-1": [state(0, 2, "leader", 1), silence(1000)], "node-2": [state(1700, 3, "leader", 2)]}
        self.assertEqual(analyze.failover_metrics(events, T0 + 2 * S)["failovers"], 0)

    def test_only_a_committed_result_counts_as_the_first_commit(self):
        m = self.failover(
            [state(0, 2, "leader", 1), silence(1000)],
            [state(1700, 3, "leader", 2), committed(1710, outcome="duplicate"),
             committed(1720, kind="release_responder"), committed(1730)],
        )
        self.assertAlmostEqual(m["failover_to_commit_ms_mean"], 730.0)


def received(at_s=1):
    return {"ts_us": T0 + at_s * S, "ev": "request_received", "request": request(0), "attempt": 0}


class TheCopiesThatWereLost(unittest.TestCase):
    def lost_share(self, sends, received_by_node, gate=(0, 0, 0)):
        events = {"client-1": [sent(i) for i in range(sends)]}
        for node, (count, dropped) in enumerate(zip(received_by_node, gate), 1):
            events[f"node-{node}"] = [received() for _ in range(count)]
            events[f"node-{node}"].append(udp_counters(T0 + S, rx_gate_dropped=dropped))
        return analyze.udp_copy_metrics(events)["udp_copies_lost_share"]

    def test_every_copy_handled_is_nothing_lost(self):
        self.assertEqual(self.lost_share(10, (10, 10, 10)), 0.0)

    def test_a_copy_nobody_handled_is_lost(self):
        self.assertAlmostEqual(self.lost_share(10, (10, 10, 7)), 3 / 30)

    def test_a_copy_the_fault_gate_turned_away_is_not_lost(self):
        self.assertEqual(self.lost_share(10, (10, 10, 7), gate=(0, 0, 3)), 0.0)

    def test_a_retry_is_three_more_copies(self):
        events = {"client-1": [sent(0), sent(1, attempt=1)], "node-1": [received(), received()]}
        events["node-2"], events["node-3"] = [received(), received()], [received()]
        self.assertAlmostEqual(analyze.udp_copy_metrics(events)["udp_copies_lost_share"], 1 / 6)

    def test_counters_a_second_old_do_not_read_as_loss(self):
        # What the rehearsal saw: nothing lost, and the node's last counters older than the last send.
        client = [sent(i) for i in range(1000)]
        client += [udp_counters(T0, tx_requests=0), udp_counters(T0 + 60 * S, tx_requests=1000)]
        node = [received() for _ in range(1000)]
        node += [udp_counters(T0, rx_requests=0), udp_counters(T0 + 59 * S, rx_requests=990)]
        events = {"client-1": client, "node-1": node}
        self.assertEqual(analyze.udp_copy_metrics(events)["udp_copies_lost_share"], 0.0)
        self.assertAlmostEqual(analyze.udp_metrics(events, T0, 100)["udp_request_loss_share"], 0.01)

    def test_a_restarted_node_counts_every_life_of_its_gate(self):
        gate = [udp_counters(T0, rx_gate_dropped=4), udp_counters(T0 + S, rx_gate_dropped=1)]
        events = {"client-1": [sent(i) for i in range(5)], "node-1": gate}
        self.assertEqual(analyze.udp_copy_metrics(events)["udp_copies_lost_share"], 0.0)

    def test_a_log_with_nothing_to_compare_has_no_such_figure(self):
        self.assertEqual(analyze.udp_copy_metrics({"client-1": [], "node-1": []}), {})
        self.assertEqual(analyze.udp_copy_metrics({"client-1": [sent(0)]}), {})


class WhatABusyLookingRunMeans(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.run_dir = Path(self.tmp.name) / "e9" / "synthetic" / "run-1"
        write_run(self.run_dir, synthetic_run(), 1)
        _, self.m = analyze.analyse_run(self.run_dir)

    def tearDown(self):
        self.tmp.cleanup()

    def test_answers_that_sent_nobody_are_left_out(self):
        self.assertAlmostEqual(self.m["throughput_rps"], 11 / 10)
        self.assertAlmostEqual(self.m["answered_with_responder_rps"], 10 / 10)
        self.assertAlmostEqual(
            self.m["answered_with_responder_rps"],
            self.m["throughput_rps"] * (1 - self.m["no_responder_share"]),
        )

    def test_the_worst_request_is_the_longest_answered_one(self):
        # The fixture's slowest measured answer. Its 999 ms one is in the warm-up, and its give-up has no
        # first send to count from.
        self.assertEqual(self.m["worst_request_ms"], 100.0)

    def test_a_request_that_gave_up_counts_how_long_it_kept_trying(self):
        client = [sent(0), done(1, 50, 1), {"ts_us": T0 + 30 * S, "ev": "client_gave_up", "request": request(0),
                                            "attempts": 6}]
        self.assertEqual(analyze.worst_request_ms({"client-1": client}, [client[1]], [client[2]]), 30000.0)

    def test_a_provisional_answer_counts_as_a_request_too(self):
        slow = done(3, 5000, 3, provisional=True, status="provisional")
        self.assertEqual(analyze.worst_request_ms({"client-1": []}, [slow], []), 5000.0)

    def test_a_run_with_no_requests_has_no_worst(self):
        self.assertTrue(math.isnan(analyze.worst_request_ms({"client-1": []}, [], [])))

    def test_conflicts_are_counted_beside_the_provisional_answers(self):
        self.assertEqual(self.m["provisional_conflicts"], 1)

    def test_a_degraded_cell_says_what_its_phases_mix(self):
        cell = {("degraded", "300"): {"provisional_replied": (5068.0, 9.0, 3),
                                      "provisional_conflicts": (3956.0, 40.0, 3)}}
        notes = analyze.provisional_notes(cell)
        self.assertEqual(len(notes), 1)
        self.assertIn("degraded/300", notes[0])
        self.assertIn("5068 provisional answers", notes[0])
        self.assertIn("3956 provisional_conflicts", notes[0])
        self.assertIn("during_*", notes[0])

    def test_a_cell_with_no_provisional_answers_has_no_note(self):
        self.assertEqual(analyze.provisional_notes({("e1", "c"): {"provisional_replied": (0.0, 0.0, 3)}}), [])
        self.assertEqual(analyze.provisional_notes({("e1", "old"): {}}), [])

    def test_a_whole_run_has_the_new_figures(self):
        self.assertEqual(self.m["failovers"], 1)
        self.assertAlmostEqual(self.m["failover_to_leader_ms_mean"], 500.0)
        self.assertTrue(math.isnan(self.m["failover_to_commit_ms_mean"]))
        self.assertEqual(self.m["failovers_without_commit"], 1)
        self.assertIn("udp_copies_lost_share", self.m)
        # The vote alone: the old figure, kept under its old name.
        self.assertAlmostEqual(self.m["election_duration_mean_ms"], 200.0)


if __name__ == "__main__":
    unittest.main()
