#!/usr/bin/env python3
# uml_figures.py: the UML diagrams in the report's appendix, drawn from the design, to docs/report/figures/uml-*.png.
# Run with .venv/bin/python from the code folder.
import textwrap
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import Circle, Ellipse, FancyBboxPatch, Polygon, Rectangle

OUT = "docs/report/figures"
INK, MUTED, LINE = "#1a1a1a", "#555555", "#333333"
APP, MID, LIB, HEAD, NOTE, DEV = "#efe7f8", "#e3f3ea", "#eeeeee", "#e8f0fb", "#fff8dc", "#f5f5f0"
plt.rcParams.update({"font.size": 9, "font.family": "DejaVu Sans", "savefig.dpi": 170, "savefig.bbox": "tight", "savefig.pad_inches": 0.08})


def canvas(w, h):
    fig, ax = plt.subplots(figsize=(w, h))
    ax.set_xlim(0, w); ax.set_ylim(0, h); ax.axis("off"); ax.set_aspect("equal")
    return fig, ax


def text(ax, x, y, s, size=9, ha="center", va="center", color=INK, weight="normal", style="normal", family=None, z=5):
    ax.text(x, y, s, fontsize=size, ha=ha, va=va, color=color, fontweight=weight, fontstyle=style, zorder=z, family=family or "DejaVu Sans", linespacing=1.15)


def arrow(ax, p, q, head="open", dashed=False, label=None, at=0.5, off=(0, 0.1), tail=None, color=LINE, size=8, lw=1.0, ha="center"):
    ax.plot([p[0], q[0]], [p[1], q[1]], color=color, lw=lw, ls=(0, (4, 3)) if dashed else "-", zorder=2)
    def tip(a, b, kind):
        import math
        dx, dy = b[0] - a[0], b[1] - a[1]
        n = math.hypot(dx, dy) or 1
        ux, uy = dx / n, dy / n
        px, py = -uy, ux
        L, W = 0.15, 0.065
        back = (b[0] - ux * L, b[1] - uy * L)
        l, r = (back[0] + px * W, back[1] + py * W), (back[0] - px * W, back[1] - py * W)
        if kind == "open":
            ax.plot([l[0], b[0], r[0]], [l[1], b[1], r[1]], color=color, lw=lw, zorder=3)
        elif kind == "filled":
            ax.add_patch(Polygon([l, b, r], closed=True, fc=color, ec=color, zorder=3))
        elif kind == "hollow":
            ax.add_patch(Polygon([l, b, r], closed=True, fc="white", ec=color, lw=lw, zorder=3))
        elif kind in ("diamond", "odiamond"):
            mid = (b[0] - ux * L * 2, b[1] - uy * L * 2)
            ax.add_patch(Polygon([b, l, mid, r], closed=True, fc=color if kind == "diamond" else "white", ec=color, lw=lw, zorder=3))
    if head:
        tip(p, q, head)
    if tail:
        tip(q, p, tail)
    if label:
        x, y = p[0] + (q[0] - p[0]) * at + off[0], p[1] + (q[1] - p[1]) * at + off[1]
        ax.text(x, y, label, fontsize=size, ha=ha, va="center", color=INK, zorder=6, linespacing=1.1, bbox=dict(fc="white", ec="none", pad=0.6, alpha=0.9))


def boxclass(ax, x, top, w, name, attrs=(), ops=(), stereo=None, fill=HEAD, size=8):
    """A UML class: name, attributes, operations. Returns its edges and centres."""
    line = 0.165
    head = 0.27 + (0.17 if stereo else 0)
    ah = max(len(attrs), 1) * line + 0.07 if attrs is not None else 0
    oh = len(ops) * line + 0.07 if ops else 0
    h = head + ah + oh
    ax.add_patch(Rectangle((x, top - h), w, h, fc="white", ec=LINE, lw=1, zorder=3))
    ax.add_patch(Rectangle((x, top - head), w, head, fc=fill, ec=LINE, lw=1, zorder=3))
    if stereo:
        text(ax, x + w / 2, top - 0.115, stereo, size=size - 0.5, style="italic", color=MUTED)
    text(ax, x + w / 2, top - head + 0.135, name, size=size + 1, weight="bold")
    y = top - head - 0.11
    for a in attrs or []:
        text(ax, x + 0.07, y, a, size=size, ha="left", family="DejaVu Sans Mono"); y -= line
    if ops:
        ax.plot([x, x + w], [top - head - ah] * 2, color=LINE, lw=1, zorder=4)
        y = top - head - ah - 0.11
        for o in ops:
            text(ax, x + 0.07, y, o, size=size, ha="left", family="DejaVu Sans Mono"); y -= line
    return dict(l=x, r=x + w, t=top, b=top - h, cx=x + w / 2, cy=top - h / 2)


def rbox(ax, cx, cy, w, h, s, fill="white", size=8.5, pad=0.07, ec=LINE, weight="normal"):
    ax.add_patch(FancyBboxPatch((cx - w / 2 + pad, cy - h / 2 + pad), w - 2 * pad, h - 2 * pad, boxstyle=f"round,pad={pad}", fc=fill, ec=ec, lw=1, zorder=3))
    text(ax, cx, cy, s, size=size, weight=weight)


def diamond(ax, cx, cy, w, h, s, size=8):
    ax.add_patch(Polygon([(cx - w / 2, cy), (cx, cy + h / 2), (cx + w / 2, cy), (cx, cy - h / 2)], closed=True, fc=NOTE, ec=LINE, lw=1, zorder=3))
    text(ax, cx, cy, s, size=size)


def start(ax, x, y):
    ax.add_patch(Circle((x, y), 0.075, fc=LINE, ec=LINE, zorder=4))


def end(ax, x, y):
    ax.add_patch(Circle((x, y), 0.1, fc="white", ec=LINE, lw=1, zorder=4))
    ax.add_patch(Circle((x, y), 0.06, fc=LINE, ec=LINE, zorder=5))


def actor(ax, x, y, name):
    ax.add_patch(Circle((x, y + 0.27), 0.09, fc="white", ec=LINE, lw=1.1, zorder=4))
    ax.plot([x, x], [y + 0.18, y - 0.08], color=LINE, lw=1.1)
    ax.plot([x - 0.15, x + 0.15], [y + 0.08, y + 0.08], color=LINE, lw=1.1)
    ax.plot([x, x - 0.13], [y - 0.08, y - 0.28], color=LINE, lw=1.1)
    ax.plot([x, x + 0.13], [y - 0.08, y - 0.28], color=LINE, lw=1.1)
    text(ax, x, y - 0.36, name, size=8.5, va="top")


def usecase_diagram():
    fig, ax = canvas(10, 7.1)
    ax.add_patch(Rectangle((2.1, 0.15), 5.8, 6.8, fc="#fbfbfb", ec=LINE, lw=1.1))
    text(ax, 5.0, 6.77, "RescueMesh cloud, Phase 1", weight="bold", size=10)
    cases = {
        "report": (3.75, 6.15, "Report an incident"), "plan": (6.3, 6.15, "Receive a response plan"),
        "retry": (3.75, 5.35, "Retry with the same ID,\nup to 5 retries"), "status": (3.75, 4.55, "Read an incident's status"),
        "load": (3.75, 3.75, "Start or stop load programs"), "inspect": (3.75, 2.95, "See each node's status"),
        "silence": (3.75, 2.15, "Silence a node, or slow it"), "run": (3.75, 1.35, "Run an experiment\nand collect its logs"),
        "elect": (6.35, 4.95, "Elect who plans a report"), "order": (6.35, 4.05, "Order commands in\nthe shared log"),
        "fault": (6.35, 3.15, "Elect a node to fail\nfor up to 20 s"), "recover": (6.35, 2.25, "Rebuild and rejoin\nafter a failure"),
        "drop": (6.35, 1.35, "Go on without a silent node"), "alone": (6.35, 0.55, "Answer alone, provisionally"),
    }
    pos = {}
    for k, (x, y, s) in cases.items():
        wid = 2.25
        ax.add_patch(Ellipse((x, y), wid, 0.62, fc=HEAD if x < 5 else MID, ec=LINE, lw=1, zorder=3))
        text(ax, x, y, s, size=8)
        pos[k] = (x, y)
    for name, (x, y) in {"Campus user\n(person or\nsimulated user)": (0.9, 5.65), "Operator": (0.9, 3.0), "Experiment\nrunner": (0.9, 1.15)}.items():
        actor(ax, x, y, name)
    actor(ax, 9.1, 2.9, "Cloud node\n(peer)")
    def link(a, b):
        ax.plot([a[0], b[0]], [a[1], b[1]], color=LINE, lw=0.9, zorder=2)
    for k in ("report", "load"):
        link((1.15, 5.65), (pos[k][0] - 1.12, pos[k][1]))
    for k in ("inspect", "silence", "status", "load"):
        link((1.15, 3.1), (pos[k][0] - 1.12, pos[k][1]))
    for k in ("run", "load", "silence"):
        link((1.15, 1.25), (pos[k][0] - 1.12, pos[k][1]))
    for k in ("elect", "order", "fault", "recover", "drop", "alone"):
        link((8.85, 3.05), (pos[k][0] + 1.12, pos[k][1]))
    arrow(ax, (4.88, 6.15), (5.17, 6.15), dashed=True)
    text(ax, 5.03, 6.42, "«include»", size=7, style="italic", color=MUTED)
    arrow(ax, (3.75, 5.66), (3.75, 5.84), dashed=True)
    text(ax, 4.42, 5.75, "«extend»", size=7, style="italic", color=MUTED)
    arrow(ax, (4.72, 5.98), (5.42, 5.2), dashed=True, label="«include»", size=7, at=0.55, off=(0.12, 0.11))
    fig.savefig(f"{OUT}/uml-use-cases.png"); plt.close(fig)


def comp(ax, x, y, w, h, name, fill):
    ax.add_patch(Rectangle((x, y), w, h, fc=fill, ec=LINE, lw=1, zorder=3))
    text(ax, x + w / 2 , y + h / 2, name, size=8.2)
    ix, iy = x + w - 0.21, y + h - 0.18
    ax.add_patch(Rectangle((ix, iy), 0.15, 0.12, fc="white", ec=LINE, lw=0.7, zorder=4))
    ax.add_patch(Rectangle((ix - 0.035, iy + 0.02), 0.07, 0.03, fc="white", ec=LINE, lw=0.6, zorder=5))
    ax.add_patch(Rectangle((ix - 0.035, iy + 0.07), 0.07, 0.03, fc="white", ec=LINE, lw=0.6, zorder=5))
    return dict(l=x, r=x + w, t=y + h, b=y, cx=x + w / 2, cy=y + h / 2)


def component_diagram():
    """Each 'uses' line is what that file reaches through NodeContext, or its own fields, in src/node/ and src/."""
    fig, ax = canvas(10.6, 7.0)
    ax.add_patch(Rectangle((0.05, 0.05), 10.5, 6.9, fc="#fbfbfb", ec=MUTED, lw=1, ls=(0, (5, 3))))
    text(ax, 5.3, 6.78, "Cloud node: one process, whose parts share one NodeContext", weight="bold")
    W, H = 2.42, 0.86
    def c(x, y, name, uses, fill):
        ax.add_patch(Rectangle((x, y), W, H, fc=fill, ec=LINE, lw=1, zorder=3))
        text(ax, x + W / 2, y + H - 0.17, name, size=8.6, weight="bold")
        text(ax, x + W / 2, y + 0.3, uses, size=7.1, color=INK)
        ix, iy = x + W - 0.2, y + H - 0.17
        ax.add_patch(Rectangle((ix, iy), 0.14, 0.11, fc="white", ec=LINE, lw=0.7, zorder=4))
        ax.add_patch(Rectangle((ix - 0.03, iy + 0.02), 0.06, 0.025, fc="white", ec=LINE, lw=0.6, zorder=5))
        ax.add_patch(Rectangle((ix - 0.03, iy + 0.065), 0.06, 0.025, fc="white", ec=LINE, lw=0.6, zorder=5))
        return dict(l=x, r=x + W, t=y + H, b=y, cx=x + W / 2, cy=y + H / 2)
    xs = (0.2, 2.8, 5.4, 7.98)
    svc = c(xs[0], 5.65, "service", "bids, degraded, gate, group, serving,\nstate, workers, plan_work", APP)
    rel = c(xs[1], 5.65, "release", "degraded, group,\nserving, state", APP)
    pla = c(xs[2], 5.65, "planner, campus", "no network, no clock:\ncalled on the worker pool", APP)
    sta = c(xs[3], 5.65, "state (StateReader)", "CloudState behind a\nread-write lock", APP)
    bid = c(xs[0], 4.45, "bids, bidding", "group, state, workers", MID)
    tko = c(xs[1], 4.45, "takeover", "bids, serving, state,\nsuspicion", MID)
    deg = c(xs[2], 4.45, "degraded", "consensus, gate, group, peers,\nserving, state, suspicion", MID)
    fau = c(xs[3], 4.45, "faults", "degraded, gate, group,\nserving", MID)
    hbt = c(xs[0], 3.25, "heartbeat", "gate, group, peers, suspicion", MID)
    rej = c(xs[1], 3.25, "rejoin", "consensus, gate, serving", MID)
    wrk = c(xs[2], 3.25, "workers (WorkerPool)", "plain threads, bounded queue", MID)
    grp = c(xs[3], 3.25, "group (CloudGroup)", "consensus, peers", MID)
    log = c(xs[3], 2.05, "rounds (RoundsNode)", "peers, state, log:\nimplements Consensus", MID)
    raf = c(xs[2], 2.05, "raft (RaftNode), off", "openraft: implements\nConsensus", LIB)
    pee = c(xs[1], 2.05, "transport (PeerSet)", "gate; tarpc over TCP", MID)
    dat = c(xs[0], 2.05, "datagram", "gate; tarpc over UDP", MID)
    gat = c(xs[1], 0.85, "FaultGate", "checked before each call", MID)
    evt = c(xs[0], 0.85, "metrics (EventLog)", "one writer thread", MID)
    squ = c(xs[2], 0.85, "squeeze", "LZ4 block format", MID)
    lib = c(xs[3], 0.85, "libraries", "tarpc, tokio, serde, bincode", LIB)
    def use(a, b, label=None, off=(0, 0.09), at=0.5):
        arrow(ax, a, b, dashed=True, label=label, off=off, at=at, size=7)
    use((svc["cx"], svc["b"]), (bid["cx"], bid["t"]))
    use((rel["r"] - 0.2, rel["b"] + 0.0), (grp["l"], grp["t"] - 0.02), None) if False else None
    use((bid["r"], bid["cy"]), (tko["l"], tko["cy"]))
    use((grp["cx"], grp["b"]), (log["cx"], log["t"]), "Consensus", off=(0.38, 0))
    use((grp["l"] + 0.25, grp["b"]), (raf["r"] - 0.25, raf["t"]), "or", off=(-0.12, 0.05))
    use((pee["cx"], pee["b"]), (gat["cx"], gat["t"]))
    use((dat["r"], dat["b"] + 0.1), (gat["l"], gat["t"] - 0.1))
    text(ax, 5.3, 0.42, "Each box lists what that part reaches in the node's shared context or its own fields; all parts also write to the event log.\nDashed arrows: the main paths. Purple: application. Green: middleware. Grey: libraries, or off.", size=7.6, color=MUTED)
    fig.savefig(f"{OUT}/uml-components.png"); plt.close(fig)


def device(ax, x, y, w, h, name):
    d = 0.14
    ax.add_patch(Polygon([(x, y + h), (x + d, y + h + d), (x + w + d, y + h + d), (x + w, y + h)], closed=True, fc="#dcdcd4", ec=LINE, lw=1, zorder=2))
    ax.add_patch(Polygon([(x + w, y), (x + w + d, y + d), (x + w + d, y + h + d), (x + w, y + h)], closed=True, fc="#cfcfc7", ec=LINE, lw=1, zorder=2))
    ax.add_patch(Rectangle((x, y), w, h, fc=DEV, ec=LINE, lw=1, zorder=3))
    text(ax, x + w / 2, y + h - 0.13, "«device»", size=7.5, style="italic", color=MUTED)
    text(ax, x + w / 2, y + h - 0.31, name, size=9, weight="bold")


def artefacts(ax, x, y, w, rows):
    yy = y
    for r in rows:
        ax.add_patch(Rectangle((x, yy - 0.2), w, 0.27, fc="white", ec=MUTED, lw=0.8, zorder=4))
        text(ax, x + 0.07, yy - 0.065, r, size=7.4, ha="left", family="DejaVu Sans Mono")
        yy -= 0.33


def deployment_diagram():
    fig, ax = canvas(10.3, 6.35)
    xs = (0.15, 3.6, 7.05)
    names = ("Node PC 1", "Node PC 2", "Node PC 3")
    rows = ["«exe» node, tokio runtime", "«config» one TOML file", "«file» node-<id>.rounds", "«file» node-<id>.jsonl"]
    for x, n in zip(xs, names):
        device(ax, x, 3.95, 2.95, 2.1, n)
        artefacts(ax, x + 0.13, 5.35, 2.69, rows)
    for a, b in ((0, 1), (1, 2)):
        arrow(ax, (xs[a] + 3.09, 5.0), (xs[b], 5.0), head=None)
    ax.plot([xs[0] + 1.47, xs[0] + 1.47, xs[2] + 1.47, xs[2] + 1.47], [6.19, 6.32, 6.32, 6.19], color=LINE, lw=1)
    text(ax, 5.1, 6.32, "tarpc over TCP, port 7001: log streams, bids, heartbeats, fault bids", size=7.8, va="center", z=7) 
    ax.add_patch(Rectangle((2.0, 6.25), 6.2, 0.14, fc="white", ec="none", zorder=2.5))
    rows2 = ["«exe» loadgen × 3 to 10", "«exe» frontend, «exe» ctl", "«script» start-users.sh", "«config» the same TOML file", "«file» client-<id>.jsonl"]
    for x, n in zip(xs, ("Client PC 1", "Client PC 2", "Client PC 3")):
        device(ax, x, 0.25, 2.95, 2.3, n)
        artefacts(ax, x + 0.13, 1.85, 2.69, rows2)
    for x in xs:
        for y in xs:
            ax.plot([x + 1.47, y + 1.47], [2.69, 3.95], color=LINE, lw=0.6, ls=(0, (4, 3)) if False else "-", zorder=1)
    ax.add_patch(Rectangle((1.75, 3.08), 6.8, 0.5, fc="white", ec="none", zorder=2))
    text(ax, 5.15, 3.33, "«UDP, port 8001» every report and cancel to all 3 nodes, one reply from each\n«TCP, port 8001» operator tool and the page's status calls", size=7.8)
    fig.savefig(f"{OUT}/uml-deployment.png"); plt.close(fig)


def activity_diagram():
    """handle_request, decide_executor, stand_by and execute in src/node/."""
    fig, ax = canvas(10, 8.3)
    for x, w, s in ((0.1, 3.3, "On arrival: handle_request"), (3.4, 3.3, "Who plans: decide_executor"), (6.7, 3.2, "Plan and answer: execute")):
        ax.add_patch(Rectangle((x, 0.1), w, 8.1, fc="#fbfbfb", ec=MUTED, lw=0.9))
        text(ax, x + w / 2, 8.03, s, weight="bold")
    A, B, C = 1.75, 5.05, 8.3
    a = lambda p, q, **k: arrow(ax, p, q, **k)
    start(ax, A, 7.72)
    a((A, 7.64), (A, 7.42))
    rbox(ax, A, 7.2, 2.7, 0.44, "log request_received")
    dy = 0.98
    tests = ["serving?", "result on record?", "provisional stored?", "degraded mode active?", "admit(): under 16,000\nin progress?"]
    ys = [6.42 - i * dy for i in range(5)]
    a((A, 6.98), (A, ys[0] + 0.36))
    for i, (y, s) in enumerate(zip(ys, tests)):
        diamond(ax, A, y, 2.75, 0.72, s, size=7.8)
        if i < 4:
            a((A, y - 0.36), (A, ys[i + 1] + 0.36), label="yes" if i == 0 else "no", off=(0.16, 0), size=7.6)
    quiet = (A, 1.02)
    rbox(ax, A, 1.02, 2.7, 0.5, "no reply: the client's\nretry covers it", size=8)
    a((A, ys[4] - 0.36), (A, 1.27), label="no", off=(0.16, 0), size=7.6)
    a((A, 0.77), (A, 0.46))
    end(ax, A, 0.35)
    ax.plot([A - 1.375, 0.22, 0.22], [ys[0], ys[0], 1.02], color=LINE, lw=1)
    a((0.22, 1.02), (A - 1.35, 1.02))
    text(ax, 0.35, ys[0] + 0.12, "no", size=7.6)
    rbox(ax, B, ys[3], 2.8, 0.5, "degraded::plan: plan on the\nworker pool, store it", size=8)
    a((A + 1.375, ys[3]), (B - 1.4, ys[3]), label="yes", off=(0, 0.1), size=7.6)
    rbox(ax, B, ys[4], 2.8, 0.5, "run_round: send bid,\nwait out the 60 ms window", size=8)
    a((A + 1.375, ys[4]), (B - 1.4, ys[4]), label="yes", off=(0, 0.1), size=7.6)
    a((B, ys[4] - 0.25), (B, 1.98))
    diamond(ax, B, 1.62, 2.1, 0.72, "i_won, and no\nresult yet?", size=7.8)
    a((B, 1.26), (B, 1.0), label="no: stand_by", off=(0.5, 0), size=7.6)
    diamond(ax, B, 0.62, 3.05, 0.76, "suspected or past backstop,\nbefore result or 2 s?", size=7.4)
    ax.plot([B - 1.525, 3.52, 3.52], [0.62, 0.62, 1.02], color=LINE, lw=1)
    a((3.52, 1.02), (A + 1.35, 1.02), label="no", off=(0.1, 0.1), size=7.6)
    plan = rbox(ax, C, 1.62, 2.75, 0.56, "begin_executing, then plan on\nthe worker pool (too old: nobody\nsent; pool full: no reply)", size=7.2)
    a((B + 1.05, 1.62), (C - 1.375, 1.62), label="yes", off=(0, 0.1), size=7.6)
    ax.plot([B + 1.525, 6.78, 6.78], [0.62, 0.62, 1.5], color=LINE, lw=1)
    a((6.78, 1.5), (C - 1.375, 1.5))
    text(ax, 6.62, 0.75, "yes", size=7.6)
    rbox(ax, C, 2.65, 2.75, 0.56, "send_ordered(CommitResult):\nnext batch, saved, sent to peers", size=7.6)
    a((C, 1.9), (C, 2.37))
    rbox(ax, C, 3.65, 2.75, 0.56, "applied in (round, node) order;\nfirst result per ID wins", size=7.6)
    a((C, 2.93), (C, 3.37))
    rbox(ax, C, 4.65, 2.75, 0.5, "save_order: Applied record\non disk", size=7.6)
    a((C, 3.93), (C, 4.4))
    rbox(ax, C, 5.65, 2.75, 0.5, "send_done, if still serving", size=7.8)
    a((C, 4.9), (C, 5.4))
    rbox(ax, C, ys[3] + 0.0, 0.01, 0.01, "") if False else None
    a((A + 1.375, ys[1]), (C - 1.375, 5.75), label="yes: same answer", at=0.35, off=(0, 0.12), size=7.6)
    rbox(ax, C, 6.6, 2.75, 0.5, "send_provisional", size=7.8)
    ax.plot([B + 1.4, 6.72, 6.72], [ys[3], ys[3], 6.5], color=LINE, lw=1)
    a((6.72, 6.5), (C - 1.375, 6.5))
    a((A + 1.375, ys[2]), (6.72, 6.5), None) if False else None
    ax.plot([A + 1.375, 6.6], [ys[2], ys[2]], color=LINE, lw=1, zorder=2)
    ax.plot([6.6, 6.6], [ys[2], 6.68], color=LINE, lw=1, zorder=2)
    a((6.6, 6.68), (C - 1.375, 6.68))
    text(ax, 4.2, ys[2] + 0.12, "yes: same answer", size=7.6)
    a((C, 6.85), (C, 7.25)); a((C, 5.9), (C, 6.35)) if False else None
    end(ax, C, 7.36)
    ax.plot([C + 1.375, C + 1.5, C + 1.5], [5.65, 5.65, 7.36], color=LINE, lw=1)
    a((C + 1.5, 7.36), (C + 0.11, 7.36))
    fig.savefig(f"{OUT}/uml-activity.png"); plt.close(fig)


def relation(ax, a, b, kind="assoc", label=None, m1=None, m2=None, off=(0, 0.1), at=0.5):
    if kind == "comp":
        arrow(ax, b, a, head="diamond", label=label, off=off, at=1 - at)
    elif kind == "aggr":
        arrow(ax, b, a, head="odiamond", label=label, off=off, at=1 - at)
    elif kind == "real":
        arrow(ax, a, b, head="hollow", dashed=True, label=label, off=off, at=at)
    elif kind == "dep":
        arrow(ax, a, b, head="open", dashed=True, label=label, off=off, at=at)
    else:
        arrow(ax, a, b, head="open", label=label, off=off, at=at)
    if m2:
        text(ax, b[0] + 0.1, b[1] + (0.1 if b[1] < a[1] else -0.1), m2, size=7.5, ha="left")


def class_client():
    """src/client.rs, src/bin/loadgen.rs, src/bin/frontend.rs, src/proto.rs."""
    fig, ax = canvas(10.2, 7.25)
    prog = boxclass(ax, 0.1, 7.15, 2.35, "loadgen", ["slots: Semaphore(20,000)"], ["open_loop(...)", "closed_loop(...)"], stereo="«program»", fill=APP)
    page = boxclass(ax, 0.1, 5.75, 2.35, "frontend: App", ["client: MeshClient", "load: Arc<Mutex<LoadBox>>"], ["route(request)", "report(query)", "status(query)", "load_start(query)"], stereo="«program»", fill=APP)
    mesh = boxclass(ax, 3.0, 7.15, 3.4, "MeshClient", ["id: ClientId", "retry, retry_max: Duration", "max_retries: u32  (5)", "call_budget: Duration", "log: EventLog", "next_seq: AtomicU64", "nodes: Vec<ClientRpcClient>", "counters: Arc<UdpCounters>", "retry_budget: Mutex<RetryBudget>", "retries_skipped: AtomicU64"], ["new(id, config, log)", "submit(incident)", "submit_from(incident, started)", "may_send(attempt): bool", "group_send(deadline, request)", "finished(reply, started, attempts)"], fill=MID)
    budget = boxclass(ax, 7.0, 7.15, 3.05, "RetryBudget", ["thousandths: u64", "refilled: Instant"], ["new(now)", "first_send(now)", "may_retry(now): bool", "refill(now)"], fill=MID)
    stub = boxclass(ax, 7.0, 5.3, 3.05, "ClientRpcClient", None, ["submit(ctx, request)", "from datagram_client(addr, ...):", "its own UDP socket"], stereo="«generated by tarpc»", fill=LIB)
    req = boxclass(ax, 0.1, 2.75, 2.2, "ClientRequest", ["id: RequestId", "attempt: u32", "age_ms: u64", "incident: Incident"], stereo="«message»")
    rep = boxclass(ax, 2.6, 2.75, 2.2, "ClientReply", ["id: RequestId", "from: NodeId", "body: ReplyBody"], stereo="«message»")
    body = boxclass(ax, 5.1, 2.75, 2.25, "ReplyBody", ["Done(CommittedResult)", "Expired(CommittedResult)", "Provisional(Plan)", "Rejected(String)"], stereo="«enum»")
    sub = boxclass(ax, 7.65, 2.75, 2.4, "Submitted", ["id, from, executor", "plan: Plan", "committed: Option<...>", "provisional: bool", "status, latency", "attempts: u32"], stereo="«returned»")
    err = boxclass(ax, 5.1, 1.0, 2.25, "ClientError", ["GaveUp { attempts }", "Connect, Timeout, Closed"], stereo="«enum»", size=7.4)
    relation(ax, (prog["r"], 6.65), (mesh["l"], 6.65), "assoc", "1", off=(0, 0.1))
    relation(ax, (page["r"], 4.95), (mesh["l"], 4.95), "assoc", "1", off=(0, 0.1))
    relation(ax, (budget["l"], 6.55), (mesh["r"], 6.55), "comp", "1", off=(0, 0.1))
    relation(ax, (stub["l"], 4.55), (mesh["r"], 4.55), "comp", "1 per node", off=(0, 0.1))
    relation(ax, (mesh["l"] + 0.4, mesh["b"]), (req["cx"] + 0.3, req["t"]), "dep", "sends; a retry\nkeeps the ID", off=(-0.62, 0.0))
    relation(ax, (mesh["cx"] - 0.6, mesh["b"]), (rep["cx"], rep["t"]), "dep", "receives", off=(0.34, 0))
    relation(ax, (rep["r"], 2.25), (body["l"], 2.25), "assoc", "1", off=(0, 0.1))
    relation(ax, (mesh["r"] - 0.4, mesh["b"]), (sub["cx"] - 0.7, sub["t"]), "dep", "returns", off=(0.3, 0.05))
    ax.plot([mesh["cx"] + 0.55, mesh["cx"] + 0.55], [mesh["b"], 2.92], color=LINE, lw=1, ls=(0, (4, 3)))
    arrow(ax, (4.95, 2.92), (4.95, 0.5), head=None, dashed=True)
    arrow(ax, (mesh["cx"] + 0.55, 2.92), (4.95, 2.92), head=None, dashed=True)
    arrow(ax, (4.95, 0.5), (err["l"], 0.5), dashed=True, label="after 5 retries", off=(-1.0, 0.0))
    fig.savefig(f"{OUT}/uml-class-client.png"); plt.close(fig)


def class_log():
    """src/rounds.rs, src/raft/mod.rs, src/proto.rs."""
    fig, ax = canvas(10.6, 9.0)
    cons = boxclass(ax, 0.1, 8.9, 3.0, "Consensus", None, ["propose(cmd), propose_local(cmd)", "handle_rpc(from, rpc)", "handle_rounds(from, message)", "is_alone(): bool", "status(), leader_commit()", "read_index(), read_index_local()", "wait_applied(index, timeout)", "set_elect(on), set_quiet(on)", "log_work(), shutdown()"], stereo="«trait»", fill=LIB, size=7.4)
    node = boxclass(ax, 0.1, 6.5, 3.0, "RoundsNode", ["inner: Arc<Inner>", "tasks: Mutex<Vec<JoinHandle>>"], ["start(id, config, peers, log)", "submit(cmd)"], fill=MID, size=7.4)
    inner = boxclass(ax, 0.1, 5.1, 3.0, "Inner", ["id, peers, log, state", "core: Mutex<Core>", "saver: mpsc sender of SaveJob", "applied, contact: watch senders", "alone: AtomicBool, wake: Notify", "stats, round, suspect_after", "propose_timeout, rpc_timeout"], ["save(batches)", "frontier(core)", "apply_ready(core), apply_batch(...)", "save_order(core), replay(path)", "install_drop(...), hold(...)", "add_alone(...), add_member(...)", "start_log_again(core)", "on_message, on_stream, on_drop", "on_catch_up, on_join"], fill=MID, size=7.2)
    tasks = boxclass(ax, 0.1, 1.28, 3.0, "tasks of a RoundsNode", ["run_saver, run_emitter", "run_sender (one per peer)", "run_detector, run_catch_up", "run_io_report"], stereo="«async fn»", fill=MID, size=7.4)
    core = boxclass(ax, 3.6, 8.9, 3.2, "Core", ["members: BTreeSet<NodeId>", "view: u64, joining: bool", "my_round, max_seen: u64", "waiting: Vec<Waiting>", "answers: BTreeMap<u64, ...>", "covered, acked, sent:", "  BTreeMap<NodeId, u64>", "streams: BTreeMap<NodeId, BTreeMap>", "pending: BTreeMap<(round, node), ..>", "ordered: Vec<Arc<RoundBatch>>", "applied_round, index: u64", "last_heard: BTreeMap<NodeId, Instant>", "promise: Option<(u64, NodeId)>", "granted: bool, drops: BTreeMap", "adding: Option<NodeId>", "awaiting: BTreeMap<NodeId, (u64, u64)>", "marks, ready: Vec<...>", "started: Instant"], fill=MID, size=7.0)
    msg = boxclass(ax, 3.6, 5.1, 3.2, "RoundsMsg", ["Stream { after, through, batches }", "Drop { view, target, have }", "DropCommit { view, target, cut, batches }", "CatchUp { next, last }", "Join", "Applied"], stereo="«enum, peer call»", size=7.0)
    rep = boxclass(ax, 3.6, 3.25, 3.2, "RoundsReply", ["Have { have }", "NotMember { view }", "NotYet", "Granted { view, have, batches }", "Refused { view }", "Done", "Entries { batches, total, applied_round,", "  view, members, starts, held }", "Joining { total, view }", "Diverged", "Applied { index }"], stereo="«enum, peer reply»", size=7.0)
    batch = boxclass(ax, 7.3, 8.9, 3.2, "RoundBatch", ["node: NodeId", "round: u64", "commands: Vec<Command>", "control: Option<RoundControl>"], stereo="«record»", size=7.4)
    ctrl = boxclass(ax, 7.3, 7.15, 2.75, "RoundControl", ["Add { node, start_round, view }"], stereo="«enum»", size=7.4)
    cmd = boxclass(ax, 7.3, 6.1, 3.2, "Command", ["RegisterResponders { responders }", "CommitResult(ResultProposal)", "ReleaseResponder { responder, request }"], stereo="«enum»", size=7.2)
    rec = boxclass(ax, 7.3, 4.6, 3.2, "LogRecord", ["Batch(RoundBatch)", "Applied { node, round }", "Reset"], stereo="«enum, a record in node-<id>.rounds»", size=7.2)
    relation(ax, (node["cx"], node["t"]), (cons["cx"], cons["b"]), "real", "implements", off=(0.5, 0))
    relation(ax, (inner["cx"], inner["t"]), (node["cx"], node["b"]), "comp", None)
    relation(ax, (core["l"], 6.9), (inner["r"], 4.7), "comp", "1", off=(0.1, 0.13))
    relation(ax, (tasks["cx"], tasks["t"]), (tasks["cx"], inner["b"]), "dep", "use", off=(0.22, 0))
    relation(ax, (inner["r"], 4.2), (msg["l"], 4.2), "dep", "handles", off=(0, 0.1))
    relation(ax, (inner["r"], 2.8), (rep["l"], 2.2), "dep", "answers", off=(0, 0.13))
    relation(ax, (batch["l"], 8.1), (core["r"], 8.1), "aggr", "*", off=(0, 0.1))
    relation(ax, (ctrl["cx"], ctrl["t"]), (ctrl["cx"], batch["b"]), "comp", "0..1", off=(0.25, 0))
    arrow(ax, (10.28, batch["b"]), (10.28, cmd["t"]), head="open", tail="diamond", label="*", off=(0.1, 0.45), size=8)
    arrow(ax, (rec["r"] - 0.2, rec["t"]), (cmd["r"] - 0.2, cmd["b"]), head=None) if False else None
    text(ax, 8.9, 3.25, "Replaying a file's Applied records in\norder, using its Batch records, rebuilds\nwhat the node applied. The other shared\nlog behind Consensus is RaftNode.", size=7.8, color=MUTED, va="top")
    fig.savefig(f"{OUT}/uml-class-log.png"); plt.close(fig)


def class_node():
    """src/node/mod.rs, bids.rs, bidding.rs, workers.rs, group.rs, degraded.rs."""
    fig, ax = canvas(10.4, 6.9)
    ctx = boxclass(ax, 3.6, 6.8, 3.2, "NodeContext", ["id: NodeId", "config: Arc<Config>", "consensus: Arc<dyn Consensus>", "state: StateReader", "gate: FaultGate", "peers: Arc<PeerSet>", "group: Arc<CloudGroup>", "workers: WorkerPool", "log: EventLog", "suspicion: Mutex<Suspicion>", "serving: AtomicBool", "bids: BidState", "fault_election: FaultElection", "plan_work: AtomicU32", "degraded: DegradedState"], fill=MID, size=7.4)
    grp = boxclass(ax, 0.1, 6.8, 2.95, "CloudGroup", ["me, all, peers, consensus, log", "view: Mutex<View>"], ["view(), term()", "send(...), probe(...)", "send_ordered(cmd)"], fill=MID, size=7.4)
    wrk = boxclass(ax, 0.1, 5.0, 2.95, "WorkerPool", ["inner: Arc<Inner>"], ["new(threads, queue_capacity)", "run(job): Result<T, Overloaded>", "load(): PoolLoad"], fill=MID, size=7.4)
    deg = boxclass(ax, 0.1, 3.5, 2.95, "DegradedState", ["active: AtomicBool", "table: Mutex<Table>"], ["is_active()", "stored(request): Option<Plan>", "pending(): usize"], fill=MID, size=7.4)
    tab = boxclass(ax, 0.1, 1.85, 2.95, "Table", ["pending: BTreeMap<RequestId, ...>", "holds: BTreeMap<ResponderId, ...>", "next_purge: Option<Instant>"], ["store(...), free(all, now)"], fill=MID, size=7.4)
    bid = boxclass(ax, 7.35, 6.8, 2.95, "BidState", ["rounds: Mutex<BidRounds>", "service: Mutex<ServiceTime>", "cpu_permille: AtomicU32", "seen, executing, outgoing", "in_progress: AtomicU32", "refused: AtomicU64"], ["admit(): Option<InProgress>", "begin_executing(request)", "saw_bid(node, score)"], fill=MID, size=7.2)
    rou = boxclass(ax, 7.35, 4.35, 2.95, "BidRounds", None, ["new(me, bid_window)", "open(key, now), add_bid(...)", "close(key, now): ClosedRound", "ranked_bids(key), bids_seen(key)", "request_decided(request, now)", "just_won(mode)", "forget_older_than(now, grace)"], stereo="«bidding.rs: no clock read»", fill=HEAD, size=7.2)
    sco = boxclass(ax, 7.35, 2.2, 2.95, "functions in bidding.rs", None, ["load_score(inputs, variant): u64", "pick_winner(bids), ranking(bids)", "executor(ranking, suspected)"], stereo="«pure»", fill=HEAD, size=7.2)
    st = boxclass(ax, 3.6, 2.55, 3.2, "StateReader", ["inner: Arc<RwLock<CloudState>>"], ["with(closure)", "apply_entry(index, group, after)", "reset()"], fill=APP, size=7.4)
    relation(ax, (grp["r"], 6.2), (ctx["l"], 6.2), "assoc", "1", off=(0, 0.1)) if False else None
    relation(ax, (grp["r"], 6.2), (ctx["l"], 6.2), "comp", None)
    relation(ax, (wrk["r"], 4.5), (ctx["l"], 4.5), "comp", None)
    relation(ax, (deg["r"], 3.15), (ctx["l"], 3.95), "comp", None)
    relation(ax, (tab["cx"], tab["t"]), (deg["cx"], deg["b"]), "comp", None)
    relation(ax, (bid["l"], 6.2), (ctx["r"], 6.2), "comp", None)
    relation(ax, (rou["cx"], rou["t"]), (bid["cx"], bid["b"]), "comp", None)
    relation(ax, (bid["l"] + 0.05, bid["b"]), (rou["l"] + 0.05, rou["t"]), "dep", None) if False else None
    arrow(ax, (bid["l"] + 0.25, rou["b"]), (bid["l"] + 0.25, sco["t"]), dashed=True, label="uses", off=(0.2, 0), size=7.5)
    relation(ax, (st["cx"], st["t"]), (ctx["cx"], ctx["b"]), "comp", None)
    fig.savefig(f"{OUT}/uml-class-node.png"); plt.close(fig)


def sequence(name, parts, steps, w=10.2, step=0.42, head=0.55):
    n = len(parts)
    h = head + sum(s.get("h", step) for s in steps) + 0.35
    fig, ax = canvas(w, h)
    xs = [w * (i + 0.5) / n for i in range(n)]
    top = h - 0.12
    for x, p in zip(xs, parts):
        hw = max(0.8, 0.052 * len(p) + 0.1)
        ax.add_patch(Rectangle((x - hw, top - 0.38), 2 * hw, 0.38, fc=HEAD, ec=LINE, lw=1, zorder=4))
        text(ax, x, top - 0.19, p, size=8.5, weight="bold")
    y = top - 0.38 - 0.25
    alive = {i: True for i in range(n)}
    last = {i: top - 0.38 for i in range(n)}
    seg = {i: [] for i in range(n)}
    for s in steps:
        k = s["k"]
        sh = s.get("h", step)
        if k == "msg":
            a, b = xs[s["a"]], xs[s["b"]]
            arrow(ax, (a, y), (b, y), head="open" if s.get("reply") else "filled", dashed=bool(s.get("reply")), label=s["t"], off=(0, 0.1), size=7.8, color="#a33" if s.get("lost") else LINE)
        elif k == "self":
            x = xs[s["a"]]
            ax.add_patch(FancyBboxPatch((x - s.get("w", 1.45), y - sh * 0.36), 2 * s.get("w", 1.45), sh * 0.72, boxstyle="round,pad=0.02", fc=s.get("fill", NOTE), ec=LINE, lw=0.8, zorder=4))
            text(ax, x, y, s["t"], size=7.7)
        elif k == "kill":
            x = xs[s["a"]]
            ax.plot([x - 0.14, x + 0.14], [y - 0.14, y + 0.14], color="#b22", lw=2, zorder=6)
            ax.plot([x - 0.14, x + 0.14], [y + 0.14, y - 0.14], color="#b22", lw=2, zorder=6)
            text(ax, x + 0.22, y, s.get("t", "kill -9"), size=7.8, ha="left", color="#b22")
            seg[s["a"]].append((last[s["a"]], y)); alive[s["a"]] = False
        elif k == "back":
            x = xs[s["a"]]
            alive[s["a"]] = True; last[s["a"]] = y + 0.1
        elif k == "span":
            ax.add_patch(Rectangle((0.12, y - sh * 0.38), w - 0.24, sh * 0.76, fc=s.get("fill", "#f2f2ee"), ec=MUTED, lw=0.7, zorder=1.5))
            text(ax, w / 2, y, s["t"], size=7.8, color=INK, z=5)
        y -= sh
    for i in range(n):
        if alive[i]:
            seg[i].append((last[i], y + 0.05))
        for a, b in seg[i]:
            ax.plot([xs[i], xs[i]], [a, b], color=MUTED, lw=0.9, ls=(0, (3, 3)), zorder=1)
    fig.savefig(f"{OUT}/{name}.png"); plt.close(fig)


def sequences():
    m = lambda a, b, t, **k: dict(k="msg", a=a, b=b, t=t, **k)
    sf = lambda a, t, **k: dict(k="self", a=a, t=t, **k)
    sequence("uml-seq-two-killed", ["Clients", "Node 1", "Node 2", "Node 3"], [
        m(0, 3, "report, sent to all 3"),
        dict(k="kill", a=1), dict(k="span", t="Nodes 2 and 3 agree to drop node 1, in about 1 s, and go on committing", h=0.36),
        dict(k="kill", a=2),
        sf(3, "2 s with no peer: answers alone", fill=MID, w=1.18, h=0.4),
        m(0, 3, "report"), m(3, 0, "provisional plan", reply=True),
        dict(k="back", a=1), sf(1, "restart: replay own file", w=1.1, h=0.4),
        m(1, 3, "CatchUp { next, last }"), m(3, 1, "Entries: up to 4,000 commands; repeats to within 200 entries", reply=True),
        m(1, 3, "Join"), sf(3, "node 2 silent: drops node 2,\ntakes node 1 back, no vote", fill=MID, w=1.2, h=0.56),
        m(3, 1, "Done", reply=True),
        m(1, 3, "CatchUp"), m(3, 1, "Entries: starts = (node 1, start, base), held", reply=True),
        sf(1, "member again: sends from start", w=1.25, h=0.4),
        m(1, 3, "Stream { after = start − 1 }"), m(3, 1, "Stream: batches after base", reply=False),
        dict(k="span", t="Commits start again with nodes 1 and 3. Node 3 replays its provisional answers through the log", h=0.36, fill=MID),
        dict(k="back", a=2), sf(2, "restart: replay own file", w=1.1, h=0.4),
        m(2, 3, "CatchUp, then Join"), sf(3, "adds node 2 through an\nAdd in its next batch", fill=MID, w=1.2, h=0.56),
        m(3, 2, "Entries: starts = (node 2, start, start − 1)", reply=True),
        dict(k="span", t="All three are members, and end with one digest", h=0.36, fill=MID),
    ])
    sequence("uml-seq-all-killed", ["Node 1", "Node 2", "Node 3"], [
        dict(k="kill", a=0, t="kill -9", h=0.0), dict(k="kill", a=1, t="kill -9", h=0.0), dict(k="kill", a=2, t="kill -9"),
        dict(k="span", t="No node is up. Clients keep retrying, and reports give up after their time runs out", h=0.36),
        dict(k="back", a=2), sf(2, "restart: replay own file", w=1.2, h=0.4),
        dict(k="back", a=1), sf(1, "restart: replay own file, a shorter log", w=1.45, h=0.4),
        m(1, 2, "CatchUp { next, last }"), m(2, 1, "Joining { total, view }", reply=True),
        m(2, 1, "CatchUp { next, last }"), m(1, 2, "Joining { total, view }", reply=True),
        sf(2, "3 s with no member: holds the longest log, so\ngoes first: members = { 3 }, view + 1", fill=MID, w=1.6, h=0.56),
        m(1, 2, "CatchUp { next, last }"), m(2, 1, "Entries: what it lacks", reply=True),
        m(1, 2, "Join"), sf(2, "alone, so takes node 2 back with no vote", fill=MID, w=1.55, h=0.4),
        m(2, 1, "Entries: starts, held", reply=True),
        dict(k="span", t="Commits start again with nodes 2 and 3", h=0.36, fill=MID),
        dict(k="back", a=0), sf(0, "restart: replay own file", w=1.2, h=0.4),
        m(0, 2, "CatchUp { next, last }"),
        m(2, 0, "Entries, or Diverged if its log is not a start of this one", reply=True),
        sf(0, "on Diverged: write Reset,\ntake the log again", w=1.2, h=0.56),
        m(0, 2, "Join"), sf(2, "two members up: an Add in the next batch", fill=MID, w=1.55, h=0.4),
        dict(k="span", t="All three are members, and end with one digest", h=0.36, fill=MID),
    ], w=10.2)
    sequence("uml-seq-retries", ["Client middleware", "Node 1", "Node 2", "Node 3"], [
        sf(0, "new ID, attempt 0,\nearns 0.2 of a retry", fill=MID, w=1.0, h=0.54),
        m(0, 1, "submit, UDP"), m(0, 2, "submit"), m(0, 3, "submit"),
        dict(k="span", t="No answer: wait 2 s, ±20%", h=0.32),
        sf(0, "budget? spend 1:\nattempt 1, same ID", fill=MID, w=1.0, h=0.54), m(0, 3, "submit, to all 3 nodes"),
        dict(k="span", t="No answer: wait 4 s", h=0.32),
        sf(0, "attempt 2", fill=MID, w=0.55, h=0.34), m(0, 3, "submit, to all 3 nodes"),
        dict(k="span", t="No answer: wait 6 s, then attempts 3, 4 and 5, each after 6 s. With no budget a retry is not sent, and its time still passes", h=0.32),
        dict(k="span", t="A reply to any attempt, from any node, ends this: first answer wins, and the others are cancelled", h=0.32, fill=MID),
        dict(k="gap", h=0.1), sf(0, "no reply: client_gave_up,\nattempts = 6, error", fill="#fbe5e0", w=1.05, h=0.56),
    ], step=0.4)


def state_box(ax, x, y, w, h, title, body=None, fill=HEAD):
    ax.add_patch(FancyBboxPatch((x - w / 2 + 0.08, y - h / 2 + 0.08), w - 0.16, h - 0.16, boxstyle="round,pad=0.08", fc=fill, ec=LINE, lw=1, zorder=3))
    if body:
        text(ax, x, y + h / 2 - 0.18, title, size=8.8, weight="bold")
        ax.plot([x - w / 2, x + w / 2], [y + h / 2 - 0.33] * 2, color=LINE, lw=0.8, zorder=4)
        text(ax, x, y - 0.14, body, size=7.6)
    else:
        text(ax, x, y, title, size=8.8, weight="bold")


def state_provisional():
    fig, ax = canvas(10, 4.75)
    start(ax, 0.3, 3.7)
    state_box(ax, 2.0, 3.7, 2.5, 0.95, "Stored", "plan kept in this node's\nmemory, responder held\nlocally", MID)
    state_box(ax, 5.1, 3.7, 2.5, 0.95, "Told", "client has a labelled\nprovisional answer", MID)
    state_box(ax, 8.2, 3.7, 2.5, 0.95, "Replaying", "up to 2,048 at a time\nthrough the shared log", MID)
    state_box(ax, 2.0, 1.1, 2.5, 0.95, "Lost", "not in any file:\nnever recorded", "#fbe5e0")
    state_box(ax, 5.1, 1.1, 2.5, 0.95, "Recorded, different", "provisional_conflict is\nlogged; client not told", NOTE)
    state_box(ax, 8.2, 1.1, 2.5, 0.95, "Recorded, same", "the record says what the\nclient was told", HEAD)
    arrow(ax, (0.38, 3.7), (0.75, 3.7), label=None)
    text(ax, 0.75, 4.42, "left alone, plans a report", size=7.8)
    arrow(ax, (3.25, 3.7), (3.85, 3.7), label="answers", off=(0, 0.12), size=7.8)
    arrow(ax, (6.35, 3.7), (6.95, 3.7), label="can commit again", off=(0, 0.72), size=7.8)
    arrow(ax, (8.2, 3.22), (8.2, 1.58), label="committed,\nsame assignment", off=(-0.62, 0), size=7.8)
    arrow(ax, (7.4, 3.22), (5.5, 1.58), label="responder taken,\nor lost first commit", off=(-0.72, 0.12), size=7.8)
    arrow(ax, (4.0, 3.22), (2.75, 1.58), label="node killed before\nits replay", off=(-0.64, 0.12), size=7.8)
    arrow(ax, (8.75, 3.22), (8.75, 3.22)) if False else None
    ax.plot([9.45, 9.68, 9.68, 9.45], [3.85, 3.85, 3.55, 3.55], color=LINE, lw=1, zorder=2)
    arrow(ax, (9.56, 3.55), (9.45, 3.55))
    text(ax, 9.9, 4.42, "not committed yet: tried again", size=7.4, ha="right")
    for x in (2.0, 5.1, 8.2):
        arrow(ax, (x, 0.62), (x, 0.3))
        end(ax, x, 0.2)
    fig.savefig(f"{OUT}/uml-state-provisional.png"); plt.close(fig)


def state_bid():
    fig, ax = canvas(10, 3.95)
    start(ax, 0.3, 2.9)
    state_box(ax, 1.9, 2.9, 2.2, 0.85, "Open", "window of 60 ms, one\nround per attempt", HEAD)
    state_box(ax, 5.0, 2.9, 2.2, 0.85, "Closed", "bids ranked: lowest,\nthen lowest node ID", HEAD)
    state_box(ax, 8.1, 3.15, 2.5, 0.72, "Executing", "plans, and proposes", MID)
    state_box(ax, 8.1, 1.65, 2.5, 0.95, "Standing by", "watches the winner and\nthe record, up to the\nclient's 2 s wait", NOTE)
    state_box(ax, 5.0, 0.75, 2.2, 0.6, "Done", None, HEAD)
    arrow(ax, (0.38, 2.9), (0.8, 2.9), label=None)
    text(ax, 0.72, 3.62, "report reaches this node", size=7.6)
    arrow(ax, (3.0, 2.9), (3.9, 2.9), label="60 ms", off=(0, 0.11), size=7.8)
    arrow(ax, (6.1, 3.05), (6.85, 3.15), label="i won, no\nresult yet", off=(0, 0.26), size=7.4)
    arrow(ax, (6.1, 2.65), (6.85, 1.95), label="i lost, or the\nresult is in", off=(-0.28, -0.2), size=7.4)
    arrow(ax, (8.1, 2.13), (8.1, 2.79), label="winner suspected,\nor backstop ≥ 1.5 s", off=(0.0, 0.0), size=7.2)
    arrow(ax, (6.85, 1.45), (6.1, 0.95), label="result on record,\nor the wait ends", off=(-0.62, -0.1), size=7.6)
    arrow(ax, (7.0, 3.0), (6.05, 1.0)) if False else None
    ax.plot([9.35, 9.6, 9.6], [3.15, 3.15, 0.75], color=LINE, lw=1, zorder=2)
    arrow(ax, (9.6, 0.75), (6.1, 0.75), label="committed: answers the client\n(worker pool full: no reply)", off=(0.2, 0.2), size=7.4, at=0.55)
    arrow(ax, (3.9, 0.75), (3.5, 0.75))
    end(ax, 3.38, 0.75)
    fig.savefig(f"{OUT}/uml-state-bid.png"); plt.close(fig)


if __name__ == "__main__":
    usecase_diagram(); component_diagram(); deployment_diagram(); activity_diagram(); class_client(); class_log(); class_node(); sequences(); state_provisional(); state_bid()
