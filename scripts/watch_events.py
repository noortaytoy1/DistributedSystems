#!/usr/bin/env python3
"""Show what the RescueMesh programs are doing, live, in plain words.

It only reads the event logs every node and client program already writes, so it needs no rebuild
and cannot slow a node down. Python 3 standard library only.

    python3 scripts/watch_events.py runs/lab                 # every log in that folder, live
    python3 scripts/watch_events.py runs/lab --reports       # also one line per report step
    python3 scripts/watch_events.py runs/lab/node-1.jsonl    # one node only
    python3 scripts/watch_events.py runs/lab --from-start --once    # read what is there, then stop

Each machine writes only its own logs, so run this on the machine you want to watch (D221).

Owner: shared. It reads event names and fields from src/metrics.rs, so a renamed event needs a change here.
"""
import argparse
import glob
import json
import os
import sys
import time
from collections import Counter


def rid(r):
    if isinstance(r, dict):
        return f"{r.get('client')}-{r.get('seq')}"
    return str(r)


def ms(us):
    return f"{(us or 0) / 1000:.1f} ms"


def tickets(t):
    return ", ".join(f"node {k} drew {v}" for k, v in sorted(t.items()))


def rare(e):
    """Plain words for an event worth a line of its own, or None."""
    ev = e.get("ev")
    if ev == "started":
        return f"STARTED. Peers on {e.get('peer_addr')}, clients on {e.get('client_addr')}"
    if ev == "stopped":
        return "STOPPED"
    if ev == "raft_state":
        leader = e.get("leader")
        who = f"leader is node {leader}" if leader is not None else "no leader right now"
        return f"RAFT: term {e.get('term')}, I am {e.get('role')}, {who}"
    if ev == "leader_stepped_aside":
        peers = ", ".join(f"node {k} bids {v}" for k, v in sorted(e.get("peer_scores", {}).items()))
        return (
            f"TOO BUSY TO LEAD term {e.get('term')}: my load is {e.get('load')} "
            f"(my bid {e.get('score')}, my log takes {ms(e.get('log_wait_us'))}), and {peers}. "
            "Going quiet, so a normal vote picks another leader"
        )
    if ev == "leader_step_aside_ended":
        outcome = e.get("outcome")
        waited = f"{e.get('waited_ms')} ms"
        if outcome == "changed":
            return f"STEPPED ASIDE: node {e.get('new_leader')} leads term {e.get('new_term')} after {waited}. Heartbeats back on"
        if outcome == "re_elected":
            return f"STEP-ASIDE DID NOT MOVE IT: the vote picked me again, term {e.get('new_term')}, after {waited}. Heartbeats back on"
        if outcome == "timed_out":
            return f"STEP-ASIDE GAVE UP: nobody took over in {waited}. Heartbeats back on, I carry on, and I wait a full cooldown before trying again"
        return f"STEP-ASIDE STOPPED after {waited}: I went silent, stopped serving or went degraded. Heartbeats back on"
    if ev == "fault_election":
        v = e.get("victim")
        who = "called it" if e.get("called") else "joined it"
        if v is None:
            return f"FAILURE DRAW round {e.get('round')}: {who}, a number is missing, so nobody goes silent. {tickets(e.get('tickets', {}))}"
        secs = (e.get("ms") or 0) / 1000
        return f"FAILURE DRAW round {e.get('round')}: {who}. {tickets(e.get('tickets', {}))}. Highest is node {v}: silent for {secs:.1f} s"
    if ev == "fault_refused":
        return f"FAILURE DRAW: node {e.get('victim')} refused: {e.get('reason')}"
    if ev == "silenced":
        return f"I WENT SILENT for {(e.get('ms') or 0) / 1000:.1f} s. Dropping everything except ctl"
    if ev == "restored":
        return "SILENCE OVER. Catching up before serving"
    if ev == "peer_suspected":
        return f"SUSPECT: no ping from node {e.get('peer')} for {e.get('silent_ms')} ms"
    if ev == "peer_trusted":
        return f"TRUSTED: node {e.get('peer')} is back, it was suspected for {e.get('suspected_ms')} ms"
    if ev == "peer_unreachable":
        return f"CANNOT REACH node {e.get('peer')}: {e.get('error')}"
    if ev == "peer_reconnected":
        return f"RECONNECTED to node {e.get('peer')} after {e.get('down_ms')} ms"
    if ev == "group_view":
        return f"GROUP: who's alive is now nodes {e.get('members')}"
    if ev == "takeover_started":
        return f"TAKEOVER: report {rid(e.get('request'))}, node {e.get('winner')} won it but is slow or gone ({e.get('reason')}, waited {ms(e.get('waited_us'))}). I am planning it"
    if ev == "degraded_entered":
        return "ALONE: every peer is gone. Answers are PROVISIONAL from here on"
    if ev == "degraded_left":
        return f"NOT ALONE ANY MORE ({e.get('reason')}) after {(e.get('degraded_ms') or 0) / 1000:.1f} s. Replaying {e.get('provisional')} provisional answers through Raft"
    if ev == "provisional_conflict":
        return f"CONFLICT on replay: report {rid(e.get('request'))}, {e.get('reason')}"
    if ev == "restarted":
        return f"RESTARTED from the log on disk: term {e.get('term')}, last line {e.get('last_log_index')}"
    if ev == "rejoin_started":
        return f"CATCHING UP from line {e.get('applied')}. Serving nobody until current"
    if ev == "rejoin_target_lowered":
        return f"CATCH-UP target corrected: line {e.get('from')} to line {e.get('to')}"
    if ev == "rejoined":
        return f"CAUGHT UP in {ms(e.get('recovery_us'))} at line {e.get('applied')}. Serving again"
    if ev == "report_expired":
        return f"TOO OLD: report {rid(e.get('request'))} is {e.get('age_ms')} ms old, valid for {e.get('ttl_ms')} ms. Nobody sent"
    if ev == "client_gave_up":
        return f"GAVE UP: report {rid(e.get('request'))} after {e.get('attempts')} tries"
    if ev == "client_started":
        return f"CLIENT PROGRAM STARTED with {e.get('users')} users"
    if ev == "udp_socket":
        got, asked = e.get("rcvbuf_got_bytes", 0), e.get("rcvbuf_asked_bytes", 0)
        note = "" if got >= asked else " (the kernel capped it: raise net.core.rmem_max)"
        return f"UDP socket on {e.get('local_addr')}: receive buffer {got} bytes{note}"
    return None


def per_report(e):
    """Plain words for one step of one report, shown only with --reports."""
    ev = e.get("ev")
    if ev == "request_received":
        return f"report {rid(e.get('request'))}: arrived (try {(e.get('attempt') or 0) + 1})"
    if ev == "bid_sent":
        return f"report {rid(e.get('request'))}: my bid is {e.get('score')} ({e.get('queued')} waiting, {e.get('running')} running, plan takes {e.get('service_us')} us)"
    if ev == "bid_round_closed":
        w = e.get("winner")
        return f"report {rid(e.get('request'))}: bids closed, {e.get('bids_seen')} seen, winner node {w}" + (", THAT IS ME, planning it" if e.get("i_won") else ", I stand by")
    if ev == "commit":
        if e.get("kind") != "commit_result":
            return None
        how = "through the leader" if e.get("forwarded") else "here on the leader"
        return f"report {rid(e.get('request'))}: {e.get('outcome')} at log line {e.get('log_index')}, {how}, took {ms(e.get('latency_us'))}"
    if ev == "replied":
        return f"report {rid(e.get('request'))}: answered the client, {ms(e.get('latency_us'))} since it arrived"
    if ev == "provisional_replied":
        return f"report {rid(e.get('request'))}: answered PROVISIONAL, responder {e.get('responder')}"
    if ev == "overloaded":
        return f"report {rid(e.get('request'))}: REFUSED, planning queue is full ({e.get('queued')} waiting)"
    if ev == "client_sent":
        return f"report {rid(e.get('request'))}: sent to all nodes (try {(e.get('attempt') or 0) + 1})"
    if ev == "client_done":
        return f"report {rid(e.get('request'))}: answer from node {e.get('from')}, planned by node {e.get('executor')}, status {e.get('status')}, priority {e.get('priority')}, {ms(e.get('latency_us'))}, {e.get('attempts')} tries"
    return None


class Summary:
    """One line per program per second, so high load stays readable."""

    def __init__(self):
        self.count = Counter()
        self.queued = None
        self.running = None
        self.io = None
        self.last_io = None

    def add(self, e):
        ev = e.get("ev")
        if ev == "request_received":
            self.count["arrived"] += 1
        elif ev == "bid_round_closed" and e.get("i_won"):
            self.count["won"] += 1
        elif ev == "commit" and e.get("kind") == "commit_result":
            self.count[e.get("outcome", "?")] += 1
        elif ev == "replied":
            self.count["answered"] += 1
        elif ev == "overloaded":
            self.count["refused"] += 1
        elif ev == "takeover_started":
            self.count["takeovers"] += 1
        elif ev == "client_sent":
            self.count["sent"] += 1
        elif ev == "client_done":
            self.count["done"] += 1
        elif ev == "client_gave_up":
            self.count["gave up"] += 1
        elif ev == "resource_usage":
            self.queued, self.running = e.get("queued"), e.get("running")
        elif ev == "raft_io":
            self.io = e

    def line(self):
        parts = [f"{k} {v}" for k, v in sorted(self.count.items())]
        if self.queued is not None:
            parts.append(f"planning queue {self.queued}, running {self.running}")
        if self.io is not None:
            before = self.last_io or {}
            saves = self.io.get("saves", 0) - before.get("saves", 0)
            cmds = self.io.get("saved_commands", 0) - before.get("saved_commands", 0)
            if saves > 0:
                parts.append(f"{saves} disk saves holding {cmds} commands")
            self.last_io = self.io
            self.io = None
        self.count = Counter()
        self.queued = None
        return ", ".join(parts)


def find(paths):
    found = []
    for p in paths:
        if os.path.isdir(p):
            found += sorted(glob.glob(os.path.join(p, "*.jsonl")))
        else:
            found.append(p)
    return found


def main():
    ap = argparse.ArgumentParser(description="Show RescueMesh event logs live, in plain words.")
    ap.add_argument("paths", nargs="*", default=["runs/lab"], help="log files or folders, default runs/lab")
    ap.add_argument("--reports", action="store_true", help="also one line per step of every report, a lot at high load")
    ap.add_argument("--from-start", action="store_true", help="read what is already there too")
    ap.add_argument("--once", action="store_true", help="stop at the end of the logs")
    args = ap.parse_args()

    files = {}
    summaries = {}
    next_summary = time.monotonic() + 1
    if not find(args.paths):
        print(f"waiting for logs in {' '.join(args.paths)} ...", flush=True)
    first_scan = True
    while True:
        for path in find(args.paths):
            if path not in files:
                try:
                    f = open(path, encoding="utf8", errors="replace")
                except OSError:
                    continue
                if first_scan and not args.from_start:
                    f.seek(0, os.SEEK_END)
                files[path] = [f, ""]
                summaries[path] = Summary()
        first_scan = False
        got_any = False
        for path, state in files.items():
            f = state[0]
            while True:
                chunk = f.readline()
                if not chunk:
                    break
                got_any = True
                state[1] += chunk
                if not state[1].endswith("\n"):
                    continue
                text, state[1] = state[1], ""
                try:
                    e = json.loads(text)
                except ValueError:
                    continue
                summaries[path].add(e)
                words = rare(e) or (per_report(e) if args.reports else None)
                if words:
                    t = e.get("ts_us", 0) / 1e6
                    stamp = time.strftime("%H:%M:%S", time.localtime(t)) + f".{int(t * 1000) % 1000:03d}"
                    print(f"{stamp} {e.get('src', '?'):>12}  {words}", flush=True)
        now = time.monotonic()
        if now >= next_summary and not args.reports and not args.once:
            next_summary = now + 1
            for path, s in summaries.items():
                text = s.line()
                if text:
                    name = os.path.basename(path).replace(".jsonl", "")
                    print(f"{time.strftime('%H:%M:%S')}     {name:>12}  last second: {text}", flush=True)
        if args.once and not got_any:
            for path, s in summaries.items():
                text = s.line()
                if text:
                    print(f"{os.path.basename(path).replace('.jsonl', ''):>12}  totals: {text}", flush=True)
            return
        if not got_any:
            try:
                time.sleep(0.2)
            except KeyboardInterrupt:
                return


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(0)
    except BrokenPipeError:
        # The reader, such as head, went away. Nothing more to show.
        sys.stderr.close()
        sys.exit(0)
