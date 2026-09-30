#!/usr/bin/python3
"""Base event rates and delivery delay, from the free public RPC only (D17).

  scripts/measure-base.py rates   [--blocks 300]   logs per block, sampled across the last 24h
  scripts/measure-base.py latest  [--minutes 5]    canonical block interval and delivery delay
  scripts/measure-base.py pending [--minutes 3]    how often pending (Flashblocks) state changes
  scripts/measure-base.py reorgs  [--minutes 60]   how often canonical blocks are replaced, and how deep

Results go in docs/spec/verification.md. Polling bounds the timing resolution to the poll
interval, and the public endpoint rate-limits, so treat delays as upper bounds.
"""

import argparse
import json
import statistics
import time
import urllib.error
import urllib.request

RPC = "https://mainnet.base.org"
BLOCKS_PER_DAY = 24 * 3600 // 2

# M1 follows Uniswap v2 and v3 pools; v4 is here to size what comes next (M9).
TOPICS = {
    "v2 Sync": "0x1c411e9a96e071241c2f21f7726b17ae89e3cab4c78be50e062b03a9fffbbad1",
    "v2 Swap": "0xd78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822",
    "v3 Swap": "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67",
    "v3 Mint": "0x7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde",
    "v3 Burn": "0x0c396cd989a39f4459b5fa1aed6a9a8dcdbc45908acfd67e028cd568da98982c",
    "v4 Swap": "0x40e9cecb9f5f1f1c5b9c97dec2917b7ee92e57ba5563708daca94dd84ad7112f",
}

rate_limited = 0
dropped = 0
round_trips_ms = []


def rpc(method, params):
    global rate_limited, dropped
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    for attempt in range(8):
        req = urllib.request.Request(RPC, body, {"Content-Type": "application/json", "User-Agent": "omnimarket-measure/0.1"})
        try:
            sent = time.time()
            with urllib.request.urlopen(req, timeout=20) as r:
                raw = r.read()
            round_trips_ms.append((time.time() - sent) * 1000)
            out = json.loads(raw)
            if "error" in out:
                raise RuntimeError(out["error"])
            return out["result"], len(raw)
        except urllib.error.HTTPError as e:
            if e.code != 429 and e.code < 500:
                raise
            if e.code == 429:
                rate_limited += 1
            else:
                dropped += 1
            time.sleep(0.5 * 2**attempt)
        except (urllib.error.URLError, ConnectionError, TimeoutError):
            dropped += 1
            time.sleep(0.5 * 2**attempt)
    raise RuntimeError("rate-limited or disconnected too many times")


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(p / 100 * len(xs)))]


def summary(name, xs, unit=""):
    print(f"  {name:<22} p50 {pct(xs, 50):>8.1f}  p90 {pct(xs, 90):>8.1f}  p99 {pct(xs, 99):>8.1f}  max {max(xs):>8.1f}  mean {statistics.mean(xs):>8.1f} {unit}")


def rates(n):
    head = int(rpc("eth_blockNumber", [])[0], 16)
    step = BLOCKS_PER_DAY // n
    all_logs, dex, dex_bytes, all_bytes = [], [], [], []
    per_topic = {k: [] for k in TOPICS}
    for i in range(n):
        number = hex(head - i * step)
        logs, size = rpc("eth_getLogs", [{"fromBlock": number, "toBlock": number}])
        all_logs.append(len(logs))
        all_bytes.append(size)
        counts = {k: sum(1 for l in logs if l["topics"] and l["topics"][0] == t) for k, t in TOPICS.items()}
        for k, c in counts.items():
            per_topic[k].append(c)
        m1 = [l for l in logs if l["topics"] and l["topics"][0] in list(TOPICS.values())[:5]]
        dex.append(len(m1))
        dex_bytes.append(len(json.dumps(m1)))
        time.sleep(0.1)
    print(f"{n} blocks sampled evenly over the last 24h (head {head}), 2s blocks:")
    summary("all logs / block", all_logs)
    summary("M1 v2+v3 logs / block", dex)
    for k, xs in per_topic.items():
        summary(f"  {k} / block", xs)
    summary("all logs KB / block", [b / 1024 for b in all_bytes])
    summary("M1 logs KB / block", [b / 1024 for b in dex_bytes])
    print(f"  => M1 logs/s mean {statistics.mean(dex) / 2:.0f}, p99 {pct(dex, 99) / 2:.0f}; "
          f"all logs/s mean {statistics.mean(all_logs) / 2:.0f}, p99 {pct(all_logs, 99) / 2:.0f}")


def latest(minutes, poll):
    seen, delays, intervals = None, [], []
    end = time.time() + minutes * 60
    while time.time() < end:
        block, _ = rpc("eth_getBlockByNumber", ["latest", False])
        now = time.time()
        number = int(block["number"], 16)
        if seen is not None and number > seen[0]:
            # A block's timestamp is whole seconds; delay = first seen minus that timestamp.
            delays.append((now - int(block["timestamp"], 16)) * 1000)
            if number == seen[0] + 1:
                intervals.append((now - seen[1]) * 1000)
        if seen is None or number > seen[0]:
            seen = (number, now)
        time.sleep(poll)
    print(f"latest polled every {poll * 1000:.0f}ms for {minutes} min: {len(delays)} new blocks")
    summary("first seen - timestamp", delays, "ms")
    summary("interval between blocks", intervals, "ms")


def pending(minutes, poll):
    last, changes, per_block, count = None, [], {}, 0
    end = time.time() + minutes * 60
    while time.time() < end:
        block, _ = rpc("eth_getBlockByNumber", ["pending", False])
        now = time.time()
        key = (block["number"], len(block["transactions"]), block.get("hash"))
        if last is not None and key != last[0]:
            changes.append((now - last[1]) * 1000)
            per_block[block["number"]] = per_block.get(block["number"], 0) + 1
        if last is None or key != last[0]:
            last = (key, now)
        count += 1
        time.sleep(poll)
    full = list(per_block.values())[1:-1]
    print(f"pending polled every {poll * 1000:.0f}ms for {minutes} min: {count} polls, {len(changes)} changes")
    summary("time between changes", changes, "ms")
    if full:
        summary("changes per block", full)


def block_hash(number):
    block, _ = rpc("eth_getBlockByNumber", [hex(number), False])
    return block["hash"] if block else None


def reorgs(minutes, poll, rechecks):
    """Keeps every canonical hash it sees. A reorg shows up two ways: a new block whose parent
    isn't the hash we hold (walk back to find the depth), or a height whose hash has changed
    when re-read some blocks later."""
    hashes, due, changed, depths = {}, [], {}, []
    head = None
    end = time.time() + minutes * 60
    while time.time() < end:
        block, _ = rpc("eth_getBlockByNumber", ["latest", False])
        number = int(block["number"], 16)
        if head is not None and number > head + 1:
            for n in range(head + 1, number):
                hashes[n] = block_hash(n)
                due += [(n + r, n) for r in rechecks]
        if head is None or number > head:
            parent = hashes.get(number - 1)
            if parent is not None and parent != block["parentHash"]:
                depth, n = 0, number - 1
                while n in hashes and hashes[n] != block_hash(n):
                    changed.setdefault(n, "on arrival")
                    hashes[n] = block_hash(n)
                    depth, n = depth + 1, n - 1
                depths.append(depth)
                print(f"  reorg at {number}: depth {depth}")
            hashes[number] = block["hash"]
            due += [(number + r, number) for r in rechecks]
            head = number
        for at, n in [d for d in due if d[0] <= head]:
            due.remove((at, n))
            now = block_hash(n)
            if now != hashes[n]:
                changed.setdefault(n, f"re-read {at - n} blocks later")
                print(f"  height {n} changed when re-read {at - n} blocks later")
                hashes[n] = now
        time.sleep(poll)
    print(f"reorgs: latest polled every {poll * 1000:.0f}ms for {minutes} min: {len(hashes)} blocks, "
          f"re-read {', '.join(str(r) for r in rechecks)} blocks later")
    print(f"  reorgs seen on arrival: {len(depths)}" + (f", max depth {max(depths)}" if depths else ""))
    print(f"  heights whose hash changed: {len(changed)}")
    for n, how in sorted(changed.items()):
        print(f"    {n}: {how}")


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("what", choices=["rates", "latest", "pending", "reorgs"])
    p.add_argument("--blocks", type=int, default=300)
    p.add_argument("--minutes", type=float, default=5)
    p.add_argument("--poll", type=float, default=0.1)
    p.add_argument("--recheck", type=int, nargs="+", default=[10, 300], help="reorgs: re-read each height this many blocks later")
    a = p.parse_args()
    {"rates": lambda: rates(a.blocks), "latest": lambda: latest(a.minutes, a.poll), "pending": lambda: pending(a.minutes, a.poll),
     "reorgs": lambda: reorgs(a.minutes, a.poll, a.recheck)}[a.what]()
    summary("request round trip", round_trips_ms, "ms")
    print(f"  (retried: {rate_limited} HTTP 429, {dropped} dropped connections or 5xx)")
