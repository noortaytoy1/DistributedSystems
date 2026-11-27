#!/usr/bin/env python3
"""Checks analyze.py against a small log written by hand. Every number here is synthetic: this file
tests the arithmetic and says nothing about RescueMesh.

    python3 analysis/test_analyze.py
"""

import importlib.util
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


def seen(node_ts_s, user, wait_ms, kind="register", proposer=False):
    return {"ts_us": T0 + int(node_ts_s * S), "ev": "directory_seen", "user": {"client": 1, "user": user},
            "kind": kind, "proposer": proposer, "log_index": 1, "wait_us": wait_ms * 1000}


def directory_state(last_applied, digest, records=3, expired=0):
    return {"ts_us": T0, "ev": "directory_state", "last_applied": last_applied, "digest": digest,
            "records": records, "live": records, "clock_ms": 0, "expired": expired, "superseded": 0,
            "revived": 0}


class Presence(unittest.TestCase):
    def test_convergence_is_the_slowest_node_of_each_write(self):
        events = {
            "node-1": [seen(1.0, 7, 2, proposer=True), seen(5.0, 7, 4, kind="deregister"),
                       directory_state(10, 99), directory_state(20, 5, expired=2)],
            "node-2": [seen(1.001, 7, 3), seen(5.0, 7, 6, kind="deregister", proposer=True),
                       directory_state(10, 99)],
            "node-3": [seen(1.002, 7, 9), directory_state(20, 6)],
            "client-1": [{"ts_us": T0 + 2 * S, "ev": "discovered", "latency_us": 4000}],
        }
        m = analyze.presence_metrics(events, T0)
        self.assertEqual(m["presence_writes_seen_by_all"], 1)
        self.assertEqual(m["presence_writes_seen_by_some"], 1, "the deregistration reached two nodes")
        self.assertAlmostEqual(m["presence_convergence_p50_ms"], 9.0)
        self.assertEqual(m["presence_seen_count"], 5)
        self.assertAlmostEqual(m["discover_client_mean_ms"], 4.0)
        self.assertEqual(m["directory_digest_mismatches"], 1, "index 20 differs between nodes 1 and 3")
        self.assertEqual(m["directory_expired"], 2)


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


def raft_io(at_ms, gap_ms):
    return {"ts_us": T0 + at_ms * 1000, "ev": "raft_io", "max_leader_gap_us": gap_ms * 1000}


def later(events, ms):
    """A clock that runs `ms` ahead of the others changes only `ts_us`, which is all it does to a log."""
    return [dict(e, ts_us=e["ts_us"] + ms * 1000) for e in events]


class FailoverOnOneClock(unittest.TestCase):
    """FT-2 reads these, so no difference between two PCs' clocks may move them (D233). Measuring starts at T0,
    and times are in ms after it."""

    def logs(self, new_leader_ahead_ms=0, follower_ahead_ms=0):
        return {
            "node-1": [state(0, 2, "leader", 1), silence(1000), state(9000, 3, "follower", 2)],
            "node-2": later(
                [state(0, 2, "follower", 1), state(1700, 3, "candidate"), state(1710, 3, "leader", 2), committed(1720)],
                new_leader_ahead_ms,
            ),
            "node-3": later(
                [state(0, 2, "follower", 1), raft_io(900, 10), state(1705, 3, "follower"),
                 state(1712, 3, "follower", 2), raft_io(1900, 716), raft_io(2900, 12)],
                follower_ahead_ms,
            ),
        }

    def test_it_reads_the_follower_that_stayed_and_only_that(self):
        m = analyze.failover_gap_metrics(self.logs(), T0)
        self.assertAlmostEqual(m["failover_gap_ms_mean"], 716.0)
        self.assertAlmostEqual(m["failover_gap_ms_max"], 716.0)

    def test_no_clock_difference_moves_it_and_each_moves_failover_to_by_exactly_itself(self):
        for ahead_ms in (-300, -20, 20, 300):
            logs = self.logs(new_leader_ahead_ms=ahead_ms, follower_ahead_ms=-ahead_ms)
            self.assertAlmostEqual(analyze.failover_gap_metrics(logs, T0)["failover_gap_ms_max"], 716.0)
            m = analyze.failover_metrics(logs, T0)
            self.assertAlmostEqual(m["failover_to_leader_ms_mean"], 710.0 + ahead_ms)

    def test_a_clock_behind_by_more_than_the_failover_reads_as_no_new_leader_and_this_does_not(self):
        logs = self.logs(new_leader_ahead_ms=-900)
        m = analyze.failover_metrics(logs, T0)
        self.assertEqual(m["failovers_without_leader"], 1)
        self.assertTrue(math.isnan(m["failover_to_leader_ms_mean"]))
        self.assertAlmostEqual(analyze.failover_gap_metrics(logs, T0)["failover_gap_ms_max"], 716.0)

    def test_the_gap_can_be_in_the_line_just_before_the_new_leader_is_named(self):
        logs = {"node-3": [state(0, 2, "follower", 1), raft_io(1711, 716), state(1712, 3, "follower", 2),
                           raft_io(2711, 12)]}
        self.assertAlmostEqual(analyze.failover_gap_metrics(logs, T0)["failover_gap_ms_max"], 716.0)

    def test_a_power_cut_needs_no_silenced_line(self):
        logs = self.logs()
        logs["node-1"] = [state(0, 2, "leader", 1)]
        self.assertAlmostEqual(analyze.failover_gap_metrics(logs, T0)["failover_gap_ms_max"], 716.0)
        self.assertEqual(analyze.failover_metrics(logs, T0)["failovers"], 0)

    def test_a_node_coming_back_from_a_silence_is_left_out(self):
        logs = {"node-1": [state(0, 2, "follower", 2), silence(1000), state(9000, 3, "follower", 3),
                           raft_io(9500, 8000)]}
        self.assertTrue(math.isnan(analyze.failover_gap_metrics(logs, T0)["failover_gap_ms_max"]))

    def test_a_term_with_no_change_of_leader_has_no_figure(self):
        logs = {"node-3": [state(0, 2, "follower", 1), raft_io(900, 10)]}
        self.assertTrue(math.isnan(analyze.failover_gap_metrics(logs, T0)["failover_gap_ms_max"]))

    def test_a_change_in_the_warm_up_does_not_count(self):
        self.assertTrue(math.isnan(analyze.failover_gap_metrics(self.logs(), T0 + 2 * S)["failover_gap_ms_max"]))

    def test_a_change_after_the_measured_time_does_not_count(self):
        self.assertAlmostEqual(analyze.failover_gap_metrics(self.logs(), T0, 2)["failover_gap_ms_max"], 716.0)
        self.assertTrue(math.isnan(analyze.failover_gap_metrics(self.logs(), T0, 1)["failover_gap_ms_max"]))


class TheHighestAcrossRuns(unittest.TestCase):
    """A mean across runs can hide the one run that matters, and FT-2 holds for every failover (D233)."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_the_worst_run_row_says_what_a_mean_of_highests_cannot(self):
        for run, worst_ms in enumerate((2300, 2400, 13800), 1):
            logs = {"client-1": [sent(0), done(3, worst_ms, 1)], "node-1": []}
            write_run(self.root / "e9" / "synthetic" / f"run-{run}", logs, run)
        metrics = analyze.summarise(self.root)[("e9", "synthetic")]
        self.assertAlmostEqual(metrics["worst_request_ms"][0], 18500 / 3)
        highest, half, runs = metrics["worst_request_ms_worst_run"]
        self.assertEqual(highest, 13800.0)
        self.assertTrue(math.isnan(half))
        self.assertEqual(runs, 3)

    def test_one_run_over_the_limit_shows_though_the_mean_is_under_it(self):
        for run, gap_ms in enumerate((600, 650, 1300), 1):
            follower = [state(0, 2, "follower", 1), raft_io(2900, 10), state(3000, 3, "follower", 2),
                        raft_io(3900, gap_ms)]
            write_run(self.root / "e9" / "synthetic" / f"run-{run}", {"client-1": [sent(0)], "node-3": follower}, run)
        metrics = analyze.summarise(self.root)[("e9", "synthetic")]
        self.assertAlmostEqual(metrics["failover_gap_ms_max"][0], 850.0)
        self.assertEqual(metrics["failover_gap_ms_max_worst_run"][0], 1300.0)
        self.assertNotIn("client_gap_max_ms_worst_run", metrics)


def answered(at_ms):
    return {"ts_us": T0 + at_ms * 1000, "ev": "client_done"}


def started():
    return {"ts_us": T0, "ev": "client_started", "users": 1}


def first_send():
    return {"ts_us": T0, "ev": "client_sent", "request": request(0), "attempt": 0}


class TheFirstNewAnswerOnOneClock(unittest.TestCase):
    def pc_gap(self, events, warmup_s=0, measured_s=10):
        return analyze.client_pc_gap_metrics(events, warmup_s, measured_s)

    def test_one_programs_answer_ends_the_gap_for_its_pc_and_not_for_another(self):
        fine = [answered(ms) for ms in range(0, 10001, 100)]
        m = self.pc_gap({
            "client-10000": [started(), first_send()] + [answered(ms) for ms in range(0, 5001, 100)],
            "client-10001": [started(), first_send()] + [answered(ms) for ms in range(5800, 10001, 100)],
            "client-20000": [started(), first_send()] + fine,
        })
        self.assertAlmostEqual(m["client_gap_pc_max_ms"], 800.0)
        self.assertAlmostEqual(m["client_gap_pc_mean_ms"], 450.0)

    def test_a_pc_that_never_hears_again_counts_its_whole_silence(self):
        m = self.pc_gap({"client-10000": [started(), first_send(), answered(1000)]})
        self.assertAlmostEqual(m["client_gap_pc_max_ms"], 9000.0)

    def test_the_warm_up_and_what_comes_after_the_measured_time_are_left_out(self):
        answers = [answered(0), answered(3000)] + [answered(ms) for ms in range(3000, 13001, 100)] + [answered(20000)]
        m = self.pc_gap({"client-10000": [started(), first_send()] + answers}, warmup_s=3)
        self.assertAlmostEqual(m["client_gap_pc_max_ms"], 100.0)

    def test_it_is_the_same_whatever_the_other_pcs_clocks_say(self):
        one = [started(), first_send()] + [answered(ms) for ms in (0, 100, 900, 1000, 10000)]
        two = [started(), first_send()] + [answered(ms) for ms in (0, 700, 800, 10000)]
        apart = self.pc_gap({"client-10000": one, "client-20000": later(two, 400)})
        self.assertEqual(apart, self.pc_gap({"client-10000": one, "client-20000": two}))


def copy_to_a_node(at_us, seq):
    return {"ts_us": at_us, "ev": "request_received", "request": {"client": 10000, "seq": seq}, "attempt": 0}


def copy_sent(at_us, seq):
    return {"ts_us": at_us, "ev": "client_sent", "request": {"client": 10000, "seq": seq}, "attempt": 0}


class HowFarApartTheClocksWere(unittest.TestCase):
    def skew(self, node_2_ahead_us, pc_ahead_us=0):
        sends = [copy_sent(T0 + i * S + pc_ahead_us, i) for i in range(3)]
        # Copy 1 is the quickest at both nodes. The others waited behind other work.
        waits = {0: 900, 1: 200, 2: 5000}
        return analyze.clock_skew_metrics({
            "client-10000": sends,
            "node-1": [copy_to_a_node(T0 + i * S + waits[i], i) for i in range(3)],
            "node-2": [copy_to_a_node(T0 + i * S + waits[i] + node_2_ahead_us, i) for i in range(3)],
        })

    def test_a_clock_difference_reads_as_itself(self):
        self.assertAlmostEqual(self.skew(40_000)["node_clock_skew_ms"], 40.0)
        self.assertAlmostEqual(self.skew(-40_000)["node_clock_skew_ms"], 40.0)

    def test_equal_clocks_read_nothing(self):
        self.assertAlmostEqual(self.skew(0)["node_clock_skew_ms"], 0.0)

    def test_a_client_pc_reads_its_difference_plus_a_hop(self):
        self.assertAlmostEqual(self.skew(0, pc_ahead_us=-70_000)["client_clock_skew_ms"], 70.2)

    def test_a_run_with_no_copies_says_nothing(self):
        self.assertEqual(analyze.clock_skew_metrics({"client-10000": [], "node-1": []}), {})

    def test_a_report_sent_again_from_the_outbox_reads_no_skew(self):
        # One PC. A copy got through just before an outage cut the report off, and Part K sent the report again
        # 10 s later under the same ID and attempt (D404). Counted from the later send, that copy came 10 s early.
        sends = [copy_sent(T0 + S, 0), copy_sent(T0 + 11 * S, 0)]
        arrivals = [copy_to_a_node(T0 + S + 200, 0), copy_to_a_node(T0 + 11 * S + 200, 0)]
        m = analyze.clock_skew_metrics({"client-10000": sends, "node-1": arrivals, "node-2": list(arrivals)})
        self.assertAlmostEqual(m["client_clock_skew_ms"], 0.2)
        self.assertAlmostEqual(m["node_clock_skew_ms"], 0.0)


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
        # 11 answers: one refused, and one provisional whose responder did not stand on record.
        self.assertAlmostEqual(self.m["throughput_rps"], 11 / 10)
        self.assertAlmostEqual(self.m["answered_with_responder_rps"], 9 / 10)
        self.assertAlmostEqual(
            self.m["answered_with_responder_rps"],
            self.m["throughput_rps"] * (1 - self.m["no_responder_share"]) - self.m["conflicts_responder_taken"] / 10,
        )

    def test_a_provisional_answer_that_lost_first_commit_still_sent_somebody(self):
        client = [sent(0), done(3, 5, 3, provisional=True, status="provisional")]
        conflict = {"ts_us": T0 + 8 * S, "ev": "provisional_conflict", "request": request(300),
                    "reason": "lost_first_commit", "responder": 1, "winner_responder": 2, "winner_executor": 2}
        m = analyze.request_metrics({"client-1": client, "node-3": [conflict]}, 2, 10)
        self.assertAlmostEqual(m["answered_with_responder_rps"], 1 / 10)

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
        self.assertIn("failover_gap_ms_max", self.m)
        self.assertIn("client_gap_pc_max_ms", self.m)
        self.assertIn("udp_copies_lost_share", self.m)
        # The vote alone: the old figure, kept under its old name.
        self.assertAlmostEqual(self.m["election_duration_mean_ms"], 200.0)


# --- Phase 2 (D447). Measuring starts at T0 + 2 s in these cases unless they say otherwise.

PEER = "10.0.0.22:20000"


def at(at_s, ev, **fields):
    return {"ts_us": T0 + int(at_s * S), "ev": ev, **fields}


def update(origin, seq):
    return {"origin": origin, "seq": seq}


def listening():
    return at(0, "p2p_listening", addr="10.0.0.21:20000")


def p2p_sent(at_s, upd, ack="accepted", tries=1, latency_ms=5.0):
    return at(at_s, "p2p_sent", update=upd, to_addr=PEER, ack=ack, tries=tries, latency_us=int(latency_ms * 1000))


def p2p_received(at_s, upd, outcome="accepted", hops=0):
    return at(at_s, "p2p_received", update=upd, **{"from": {"client": upd["origin"], "user": 0}}, hops=hops,
              outcome=outcome)


def published(at_s, upd, targets=2):
    return at(at_s, "gossip_published", update=upd, zone=1, kind="summary", version=1, targets=targets)


def gossip_received(at_s, upd, outcome="new", hops=0, forwarded=0, age_ms=10):
    return at(at_s, "gossip_received", update=upd, zone=1, kind="summary", hops=hops, outcome=outcome,
              forwarded=forwarded, age_ms=age_ms)


def claim_write(at_s, outcome, latency_ms, kind="claim", speculative=False, claimant=1):
    return at(at_s, "claim_write", resource={"provider": 30000, "seq": 1}, claimant={"client": 10000, "user": claimant},
              quantity=1 if kind == "claim" else 0, kind=kind, outcome=outcome, speculative=speculative, log_index=1,
              latency_us=int(latency_ms * 1000))


def claims_state(last_applied, digest, reconciling=0):
    return {"ts_us": T0, "ev": "claims_state", "last_applied": last_applied, "digest": digest, "offers": 1,
            "reconciling": reconciling, "claimed": 0, "refused": 0, "resolved": 0}


def workload_started(delay_ms=500):
    return at(0, "workload_started", ordinary=8, responders=1, providers=1, offline_chance_per_min_percent=10,
              offline_for_ms=20_000, claim_delay_ms=delay_ms)


def workload_done(actions, outages=0):
    return at(12, "workload_done", actions=actions, outages=outages)


def usage(at_s, rx, tx):
    return at(at_s, "resource_usage", rss_kb=1, cpu_ticks=0, net_rx_bytes=rx, net_tx_bytes=tx, udp_drops=0,
              queued=0, running=0)


class ThePeerToPeerLink(unittest.TestCase):
    def test_delivery_latency_counts_the_sends_whose_peer_has_the_update(self):
        u = update(10000, 1)
        sender = [listening(), p2p_sent(1, u, latency_ms=999),  # in the warm-up: dropped
                  p2p_sent(3, u, latency_ms=10), p2p_sent(4, u, ack="duplicate", tries=2, latency_ms=30),
                  p2p_sent(5, u, ack="refused", latency_ms=1), p2p_sent(6, u, ack="none", tries=3, latency_ms=1500)]
        receiver = [listening(), p2p_received(3, u), p2p_received(4, u, outcome="duplicate")]
        m = analyze.p2p_metrics({"client-10000": sender, "client-20000": receiver}, T0 + 2 * S)
        self.assertEqual((m["p2p_sent"], m["p2p_delivered"], m["p2p_duplicate"], m["p2p_refused"], m["p2p_failed"]),
                         (4, 1, 1, 1, 1))
        self.assertEqual(m["p2p_latency_count"], 2, "a refusal or no ack delivered nothing")
        self.assertAlmostEqual(m["p2p_latency_mean_ms"], 20.0)
        self.assertAlmostEqual(m["p2p_latency_p99_ms"], 30.0)
        self.assertAlmostEqual(m["p2p_ack_share"], 3 / 4)
        self.assertAlmostEqual(m["p2p_retried_share"], 1 / 3)
        self.assertEqual((m["p2p_received"], m["p2p_endpoints"]), (2, 2))

    def test_a_run_without_p2p_reads_zero_and_nan(self):
        m = analyze.p2p_metrics({"client-1": [sent(0)], "node-1": []}, T0)
        self.assertEqual((m["p2p_sent"], m["p2p_endpoints"]), (0, 0))
        self.assertTrue(math.isnan(m["p2p_latency_mean_ms"]))
        self.assertTrue(math.isnan(m["p2p_ack_share"]))


class ZoneGossip(unittest.TestCase):
    """Three programs, each with a peer endpoint. Program 10000 publishes."""

    def logs(self):
        early, u1, u2, direct = update(10000, 0), update(10000, 1), update(10000, 2), update(20000, 9)
        origin = [listening(), published(1, early),  # in the warm-up: not followed
                  published(3, u1, targets=2), published(4, u2, targets=0), p2p_sent(3, u1), p2p_sent(3, u1),
                  at(3, "gossip_zone_refreshed", zone=1, programs=3, ok=True, latency_us=900)]
        second = [listening(), p2p_received(3, u1), gossip_received(3, u1, forwarded=1, age_ms=12),
                  p2p_sent(3, u1),  # its copy on to the third
                  p2p_sent(5, u1, ack="duplicate"),  # store-and-forward tried it again
                  p2p_received(3.1, u1, outcome="duplicate"),  # the endpoint knew the ID: gossip never saw it
                  p2p_sent(6, direct)]  # a user's own message to a peer, which nobody published
        third = [listening(), p2p_received(3, u1), gossip_received(3, u1, hops=1, age_ms=30),
                 p2p_received(3.2, u1), gossip_received(3.2, u1, outcome="duplicate"),  # the endpoint forgot it
                 p2p_received(1.5, early, outcome="duplicate"),
                 p2p_received(6, direct), gossip_received(6, direct)]
        return {"client-10000": origin, "client-20000": second, "client-30000": third}

    def test_duplicates_count_at_the_endpoint_and_in_gossip(self):
        m = analyze.gossip_metrics(self.logs(), T0 + 2 * S)
        self.assertEqual((m["gossip_published"], m["gossip_published_no_targets"]), (2, 1))
        self.assertEqual(m["gossip_copies_received"], 4, "the warm-up update and the direct message are left out")
        self.assertEqual(m["gossip_duplicates"], 2)
        self.assertAlmostEqual(m["gossip_duplicate_share"], 0.5)
        self.assertAlmostEqual(m["gossip_duplicates_per_new"], 1.0)
        self.assertEqual((m["gossip_received_new"], m["gossip_received_duplicate"]), (2, 1))

    def test_forwarding_overhead_is_copies_chosen_and_copies_sent_per_update(self):
        m = analyze.gossip_metrics(self.logs(), T0 + 2 * S)
        self.assertAlmostEqual(m["gossip_copies_per_update"], (2 + 0 + 1) / 2)
        self.assertAlmostEqual(m["gossip_sends_per_update"], 4 / 2, msg="a later try is a send too")

    def test_coverage_and_hops(self):
        m = analyze.gossip_metrics(self.logs(), T0 + 2 * S)
        self.assertAlmostEqual(m["gossip_reach_mean"], 1.0, msg="one update reached both others, one nobody")
        self.assertAlmostEqual(m["gossip_coverage_share"], 0.5)
        self.assertAlmostEqual(m["gossip_full_coverage_share"], 0.5)
        self.assertAlmostEqual(m["gossip_hops_mean"], 0.5)
        self.assertEqual(m["gossip_hops_max"], 1)
        self.assertAlmostEqual(m["gossip_hops_1_share"], 0.5)
        self.assertEqual(m["gossip_hops_7_share"], 0.0)
        self.assertAlmostEqual(m["gossip_age_mean_ms"], 21.0)
        self.assertEqual(m["gossip_zone_programs_max"], 3)

    def test_only_the_origins_clock_chooses_the_updates(self):
        logs = self.logs()
        # The receivers' clocks 5 s behind put all their lines before the measured time. Joined by ID, it moves
        # nothing.
        behind = dict(logs, **{s: later(logs[s], -5000) for s in ("client-20000", "client-30000")})
        self.assertEqual(analyze.gossip_metrics(behind, T0 + 2 * S), analyze.gossip_metrics(logs, T0 + 2 * S))

    def test_a_run_without_gossip_reads_zero_and_nan(self):
        m = analyze.gossip_metrics({"client-1": [sent(0)]}, T0)
        self.assertEqual(set(m), set(analyze.gossip_metrics(self.logs(), T0)))
        self.assertEqual(m["gossip_published"], 0)
        self.assertTrue(math.isnan(m["gossip_coverage_share"]))
        self.assertTrue(math.isnan(m["gossip_hops_0_share"]))


class OfflineOperation(unittest.TestCase):
    """Program 10000 goes offline at 3 s and is back at 13 s; program 20000 is away 1 s with nothing queued."""

    def logs(self):
        a, b, c = update(10000, 1), update(10000, 2), update(10000, 3)
        away = [
            at(0.5, "client_offline"), at(1, "client_online", offline_ms=500, queued=1),  # in the warm-up
            at(1.1, "outbox_flushed", sent=1, answered=1, expired=0, failed=0, reconcile_us=50_000),
            at(3, "client_offline"),
            at(3.5, "outbox_queued", request=request(1)), at(3.6, "outbox_queued", request=request(2)),
            at(4, "peer_update_held", update=a, to_addr=PEER, held=1),
            at(4, "peer_update_held", update=b, to_addr=PEER, held=2),
            at(4.5, "claim_queued", resource={"provider": 20000, "seq": 1}, claimant={"client": 10000, "user": 0}),
            at(13, "client_online", offline_ms=10_000, queued=2),
            at(13.1, "outbox_sent", request=request(1), outcome="answered", waited_ms=9_500),
            at(13.2, "outbox_sent", request=request(2), outcome="failed", waited_ms=9_600),
            at(13.4, "outbox_flushed", sent=2, answered=1, expired=0, failed=1, reconcile_us=400_000),
            at(13.6, "claims_flushed", sent=1, claimed=0, reconciling=1, other=0, failed=0, reconcile_us=200_000),
            at(13.7, "peer_update_forwarded", update=a, to_addr=PEER, ack="accepted", since_made_ms=9_700),
            at(14, "peer_update_dropped", update=c, to_addr=PEER, reason="obsolete"),  # let go at once, never held
        ]
        quiet = [at(5, "client_offline"), at(6, "client_online", offline_ms=1_000, queued=0)]
        return {"client-10000": away, "client-20000": quiet}

    def test_a_reconnect_runs_to_the_end_of_its_last_flush(self):
        m = analyze.offline_metrics(self.logs(), T0 + 2 * S)
        self.assertEqual(m["offline_count"], 2)
        self.assertAlmostEqual(m["offline_mean_ms"], 5500.0)
        self.assertEqual(m["reconcile_count"], 1, "nothing queued, nothing to reconcile")
        self.assertAlmostEqual(m["reconcile_mean_ms"], 600.0, msg="from 13.0 s to the claims flush at 13.6 s")
        self.assertAlmostEqual(m["reconcile_max_ms"], 600.0)
        self.assertEqual(m["reconcile_reports_count"], 1, "the warm-up's flush is left out")
        self.assertAlmostEqual(m["reconcile_reports_mean_ms"], 400.0)
        self.assertAlmostEqual(m["reconcile_claims_mean_ms"], 200.0)

    def test_queued_reports_claims_and_held_updates_are_counted(self):
        m = analyze.offline_metrics(self.logs(), T0 + 2 * S)
        self.assertEqual((m["outbox_queued"], m["outbox_answered"], m["outbox_failed"], m["outbox_expired"]),
                         (2, 1, 1, 0))
        self.assertAlmostEqual(m["outbox_wait_mean_ms"], 9500.0, msg="answered reports only")
        self.assertEqual((m["claims_outbox_queued"], m["claims_outbox_sent"], m["claims_outbox_reconciling"]),
                         (1, 1, 1))
        self.assertEqual((m["peer_updates_held"], m["peer_updates_forwarded"], m["peer_updates_dropped"]), (2, 1, 1))
        self.assertEqual(m["peer_updates_dropped_obsolete"], 1)
        self.assertEqual(m["peer_updates_held_at_end"], 1, "b was never sent or let go")

    def test_a_run_without_outages_reads_zero_and_nan(self):
        m = analyze.offline_metrics({"client-1": [sent(0)]}, T0)
        self.assertEqual(set(m), set(analyze.offline_metrics(self.logs(), T0)))
        self.assertEqual((m["offline_count"], m["outbox_queued"], m["peer_updates_held"]), (0, 0, 0))
        self.assertTrue(math.isnan(m["reconcile_max_ms"]))


class OffersAndClaims(unittest.TestCase):
    def logs(self):
        resource, challenger = {"provider": 30000, "seq": 1}, {"client": 10000, "user": 3}
        node1 = [
            claim_write(1, "resource_claimed", 50),  # in the warm-up: dropped
            claim_write(3, "resource_claimed", 2),
            claim_write(4, "resource_claim_refused", 4, claimant=2),
            at(4, "claim_conflict", resource=resource, claimant={"client": 10000, "user": 2}, kind="online_refused",
               log_index=5),
            claim_write(5, "resource_claim_reconciling", 6, speculative=True, claimant=3),
            at(5, "claim_conflict", resource=resource, claimant=challenger, kind="reconciliation_opened", log_index=6),
            claim_write(6, "resource_unknown", 8, claimant=4),
            claim_write(7, "resource_claim_released", 1, kind="release"),
            at(3, "resource_offered", resource=resource, provider={"client": 30000, "user": 0}, capability="Fire",
               zone=1, capacity=1, available_until_ms=0, log_index=2, latency_us=3000),
            at(9, "reconciliation_resolved", resource=resource, challenger=challenger, winner="Challenger",
               wait_ms=4200, log_index=9),
            claims_state(10, 1, reconciling=1), claims_state(20, 7, reconciling=2),
        ]
        node2 = [claims_state(10, 1, reconciling=1), claims_state(20, 8), claims_state(30, 9)]
        return {"node-1": node1, "node-2": node2}

    def test_conflicts_by_kind_and_per_claim(self):
        m = analyze.claims_metrics(self.logs(), T0 + 2 * S)
        self.assertEqual(m["claim_count"], 4, "claims only, not the release")
        self.assertEqual((m["claim_claimed"], m["claim_refused"], m["claim_reconciling"], m["claim_unknown"]),
                         (1, 1, 1, 1))
        self.assertEqual(m["claim_speculative"], 1)
        self.assertAlmostEqual(m["claim_conflicts_per_claim"], 2 / 4)
        self.assertEqual((m["claim_conflicts_online_refused"], m["claim_conflicts_reconciliation_opened"]), (1, 1))
        self.assertEqual((m["release_released"], m["release_stale"]), (1, 0))

    def test_latencies_and_the_time_in_reconciliation(self):
        m = analyze.claims_metrics(self.logs(), T0 + 2 * S)
        self.assertAlmostEqual(m["claim_mean_ms"], 5.0)
        self.assertAlmostEqual(m["offer_mean_ms"], 3.0)
        self.assertAlmostEqual(m["reconciliation_time_mean_ms"], 4000.0, msg="opened at 5 s, resolved at 9 s")
        self.assertAlmostEqual(m["reconciliation_wait_mean_ms"], 4200.0)

    def test_digests_are_compared_at_equal_indexes_and_open_entries_read_at_the_end(self):
        m = analyze.claims_metrics(self.logs(), T0 + 2 * S)
        self.assertEqual(m["claims_states_compared"], 2, "index 30 is only on node 2")
        self.assertEqual(m["claims_digest_mismatches"], 1, "index 20 differs")
        self.assertEqual(m["claims_reconciling_at_end"], 2)

    def test_a_run_without_claims_reads_zero_and_nan(self):
        m = analyze.claims_metrics({"node-1": []}, T0)
        self.assertEqual(set(m), set(analyze.claims_metrics(self.logs(), T0)))
        self.assertEqual((m["claim_count"], m["claims_reconciling_at_end"]), (0, 0))
        self.assertTrue(math.isnan(m["claim_conflicts_per_claim"]))


class TheWorkload(unittest.TestCase):
    def test_actions_are_summed_over_programs_by_outcome(self):
        events = {
            "client-10000": [workload_started(), workload_done(
                {"report:answered": 50, "claim:claimed": 3, "claim:refused": 1, "register:ok": 10,
                 "discover:not_joined": 2, "peer_received:incident_summary": 7}, outages=1)],
            "client-20000": [workload_started(), workload_done(
                {"report:answered": 30, "claim:refused": 3, "peer_send:delivered": 5})],
        }
        m = analyze.workload_metrics(events, 10)
        self.assertEqual(m["workload_report_answered"], 80)
        self.assertEqual((m["workload_claim_claimed"], m["workload_claim_refused"]), (3, 4))
        self.assertAlmostEqual(m["workload_claim_refused_share"], 4 / 7)
        self.assertEqual(m["workload_peer_received_incident_summary"], 7)
        self.assertEqual(m["workload_other"], 2, "discover:not_joined has no key of its own")
        self.assertEqual(m["workload_actions"], 104, "updates received are not actions")
        self.assertAlmostEqual(m["workload_actions_per_s"], 10.4)
        self.assertEqual((m["workload_programs"], m["workload_outages"]), (2, 1))
        self.assertEqual((m["workload_ordinary"], m["workload_responders"], m["workload_providers"]), (16, 2, 2))
        self.assertEqual(m["workload_claim_delay_ms"], 500)

    def test_a_phase_1_run_has_counts_of_zero_and_no_settings(self):
        m = analyze.workload_metrics({"client-1": [sent(0)]}, 10)
        self.assertEqual((m["workload_report_answered"], m["workload_programs"]), (0, 0))
        self.assertTrue(math.isnan(m["workload_claim_refused_share"]))
        self.assertTrue(math.isnan(m["workload_claim_delay_ms"]))
        self.assertTrue(math.isnan(m["workload_actions_per_s"]))


class TheCloudsBytes(unittest.TestCase):
    """D435. The measured time runs from T0 + 2 s to T0 + 12 s."""

    def logs(self):
        node1 = [udp_counters(T0 + 2 * S, rx_bytes=1000, tx_bytes=1000),
                 udp_counters(T0 + 12 * S, rx_bytes=11_000, tx_bytes=6000),
                 udp_counters(T0 + 30 * S, rx_bytes=999_999, tx_bytes=999_999),  # after the measured time
                 usage(2, 0, 0), usage(12, 50_000, 50_000)]
        node2 = [udp_counters(T0 + 2 * S), udp_counters(T0 + 12 * S, rx_bytes=5000)]
        client = [udp_counters(T0 + 2 * S), udp_counters(T0 + 12 * S, tx_bytes=15_000, rx_bytes=5000)]
        client += [p2p_sent(3 + i * 0.1, update(10000, i)) for i in range(19)]
        client += [p2p_sent(5, update(10000, 99), ack="none", tries=3), p2p_sent(13, update(10000, 100))]
        return {"node-1": node1, "node-2": node2, "client-10000": client}

    def test_each_rate_is_over_the_measured_time_on_its_own_clock(self):
        m = analyze.cloud_bytes_metrics(self.logs(), T0 + 2 * S, T0 + 12 * S)
        self.assertAlmostEqual(m["cloud_udp_bytes_per_s"], 1500.0 + 500.0)
        self.assertAlmostEqual(m["cloud_net_bytes_per_s"], 10_000.0)
        self.assertAlmostEqual(m["client_cloud_socket_bytes_per_s"], 2000.0)
        self.assertAlmostEqual(m["cloud_udp_bytes_over_client"], 1.0)
        self.assertAlmostEqual(m["p2p_sends_per_s"], 2.0)
        self.assertAlmostEqual(m["p2p_tries_per_s"], 2.2)

    def test_a_restarted_node_adds_no_negative_bytes(self):
        node = [udp_counters(T0 + 2 * S, rx_bytes=5000), udp_counters(T0 + 7 * S, rx_bytes=1000),
                udp_counters(T0 + 12 * S, rx_bytes=3000)]
        m = analyze.cloud_bytes_metrics({"node-1": node}, T0 + 2 * S, T0 + 12 * S)
        self.assertAlmostEqual(m["cloud_udp_bytes_per_s"], 200.0)

    def test_a_run_without_counters_has_no_rates(self):
        m = analyze.cloud_bytes_metrics({"node-1": [], "client-1": []}, T0, T0 + 10 * S)
        self.assertTrue(math.isnan(m["cloud_udp_bytes_per_s"]))
        self.assertTrue(math.isnan(m["cloud_udp_bytes_over_client"]))
        self.assertEqual(m["p2p_sends_per_s"], 0.0)


def phase2_run():
    """Two programs on two PCs and three nodes, with a little of every Phase 2 family. Warm-up 2 s, 10 s
    measured. Program 10000 goes offline from 4.5 s to 9.5 s."""
    a, b = update(10000, 1), update(10000, 2)
    resource = {"provider": 20000, "seq": 1}
    first = [
        at(0, "client_started", users=10, host="pc1", ip="10.0.0.21"), sent(0), listening(), workload_started(),
        published(3, a, targets=1), p2p_sent(3, a, latency_ms=4),
        published(4, b, targets=1), p2p_sent(4, b, ack="none", tries=3, latency_ms=1500),
        at(4, "peer_update_held", update=b, to_addr=PEER, held=1),
        at(4.5, "client_offline"), at(6, "outbox_queued", request=request(1)),
        at(9.5, "client_online", offline_ms=5000, queued=1),
        at(9.7, "outbox_sent", request=request(1), outcome="answered", waited_ms=3700),
        at(9.8, "outbox_flushed", sent=1, answered=1, expired=0, failed=0, reconcile_us=300_000),
        at(10, "peer_update_forwarded", update=b, to_addr=PEER, ack="accepted", since_made_ms=6000),
        udp_counters(T0 + 2 * S), udp_counters(T0 + 12 * S, tx_bytes=9000, rx_bytes=3000),
        workload_done({"report:answered": 20, "report:queued": 1, "claim:claimed": 1, "claim:refused": 1}, outages=1),
    ]
    second = [
        at(0, "client_started", users=10, host="pc2", ip="10.0.0.22"), sent(0), listening(), workload_started(),
        p2p_received(3, a), gossip_received(3, a, age_ms=4),
        p2p_received(10, b), gossip_received(10, b, age_ms=6000),
        udp_counters(T0 + 2 * S), udp_counters(T0 + 12 * S, tx_bytes=3000, rx_bytes=1000),
        workload_done({"report:answered": 25}),
    ]
    nodes = {f"node-{n}": [udp_counters(T0 + 2 * S), udp_counters(T0 + 12 * S, rx_bytes=4000, tx_bytes=1000),
                           claims_state(50, 77)] for n in (1, 2, 3)}
    nodes["node-1"] += [
        claim_write(3, "resource_claimed", 2), claim_write(4, "resource_claim_refused", 4, claimant=2),
        at(4, "claim_conflict", resource=resource, claimant={"client": 10000, "user": 2}, kind="online_refused",
           log_index=5),
    ]
    return {"client-10000": first, "client-20000": second, **nodes}


def outbox_files(directory):
    """Part K's outboxes as offline.rs writes them, beside the logs (D401, D414). Neither is a log."""
    (directory / "outbox-client-10000.jsonl").write_text(json.dumps(
        {"id": {"client": 10000, "seq": 7}, "incident": {"zone": 1}, "queued_at_ms": 1_000_000}) + "\n")
    (directory / "claims-client-10000.jsonl").write_text(json.dumps(
        {"resource": {"provider": 3, "seq": 81604378624}, "claimant": {"client": 10000, "user": 16},
         "quantity": 1, "at_ms": 1_000_000}) + "\n")
    (directory / "outbox-client-20000.jsonl").write_text("")


class APhase2Run(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.run_dir = self.root / "ph2users" / "100" / "run-1"
        write_run(self.run_dir, phase2_run(), 1, experiment="ph2users", cell="100", users=20,
                  client_programs_expected=2, nodes=3)
        outbox_files(self.run_dir)

    def tearDown(self):
        self.tmp.cleanup()

    def test_the_outbox_files_are_not_read_as_logs(self):
        self.assertEqual(sorted(analyze.read_events(self.run_dir)),
                         ["client-10000", "client-20000", "node-1", "node-2", "node-3"])

    def test_a_whole_run_has_every_family_and_is_kept(self):
        meta, m = analyze.analyse_run(self.run_dir)
        self.assertIsNone(analyze.run_problem(meta, m))
        self.assertEqual((m["p2p_sent"], m["p2p_failed"]), (2, 1))
        self.assertAlmostEqual(m["p2p_latency_mean_ms"], 4.0)
        self.assertEqual(m["gossip_published"], 2)
        self.assertAlmostEqual(m["gossip_coverage_share"], 1.0, msg="b got there after the outage, joined by ID")
        self.assertEqual(m["offline_count"], 1)
        self.assertAlmostEqual(m["reconcile_mean_ms"], 300.0)
        self.assertEqual((m["peer_updates_held"], m["peer_updates_forwarded"], m["peer_updates_held_at_end"]),
                         (1, 1, 0))
        self.assertAlmostEqual(m["claim_conflicts_per_claim"], 0.5)
        self.assertEqual((m["claims_states_compared"], m["claims_digest_mismatches"]), (1, 0))
        self.assertEqual(m["workload_report_answered"], 45)
        self.assertAlmostEqual(m["workload_claim_refused_share"], 0.5)
        self.assertAlmostEqual(m["cloud_udp_bytes_per_s"], 3 * 500.0)
        self.assertAlmostEqual(m["cloud_udp_bytes_over_client"], 1500.0 / 1600.0)

    def test_a_phase_1_run_gets_every_phase_2_figure_at_0_or_nan(self):
        write_run(self.root / "e9" / "synthetic" / "run-1", synthetic_run(), 1)
        _, old = analyze.analyse_run(self.root / "e9" / "synthetic" / "run-1")
        _, new = analyze.analyse_run(self.run_dir)
        families = ("p2p_", "gossip_", "offline_", "reconcile", "outbox_", "claims_", "peer_updates_", "claim_",
                    "release_", "offer_", "reconciliation_", "workload_", "cloud_", "client_cloud_")
        self.assertLessEqual({name for name in new if name.startswith(families)}, set(old))
        self.assertEqual(old["gossip_published"], 0)
        self.assertTrue(math.isnan(old["p2p_latency_mean_ms"]))

    def test_a_cell_of_runs_gets_a_worst_reconcile_row(self):
        summary = analyze.summarise(self.root)
        mean, _, runs = summary[("ph2users", "100")]["reconcile_max_ms_worst_run"]
        self.assertEqual((mean, runs), (300.0, 1))


class ThePhase2Sweeps(unittest.TestCase):
    def test_each_cell_is_placed_by_the_last_number_in_its_name(self):
        def cell(mean, half=0.0):
            return {"claim_conflicts_per_claim": (mean, half, 3)}

        summary = {
            ("ph2conflict", "2000ms"): cell(0.4, 0.1), ("ph2conflict", "0ms"): cell(0.1, float("nan")),
            ("ph2conflict", "500ms"): cell(0.2, 0.05), ("ph2conflict", "try"): cell(0.9),
            ("ph2conflict", "1000ms"): cell(float("nan")), ("ph2users", "100"): cell(0.3),
        }
        self.assertEqual(analyze.sweep_series(summary, "ph2conflict", "claim_conflicts_per_claim"),
                         [(0, 0.1, 0.0), (500, 0.2, 0.05), (2000, 0.4, 0.1)])
        self.assertEqual(analyze.sweep_series(summary, "ph2offline", "claim_conflicts_per_claim"), [])

    @unittest.skipUnless(importlib.util.find_spec("matplotlib"), "matplotlib is not installed")
    def test_a_sweep_with_runs_gets_its_figure_and_one_without_gets_none(self):
        summary = {("ph2users", cell): {"p50_ms": (10.0 * i, 1.0, 3), "throughput_rps": (100.0 * i, 5.0, 3)}
                   for i, cell in enumerate(("100", "1000", "5000"), 1)}
        with tempfile.TemporaryDirectory() as tmp:
            analyze.write_plots(summary, Path(tmp))
            made = sorted(p.name for p in Path(tmp).glob("*.png"))
        self.assertEqual(made, ["ph2users_scale.png"])


if __name__ == "__main__":
    unittest.main()
