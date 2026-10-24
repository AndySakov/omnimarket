#!/usr/bin/env python3
"""Captures real Base blocks for the venue state test (#91) into base-blocks.txt.

    crates/venues/tests/fixtures/capture.py [--rpc URL] [--to BLOCK]

Walks back from BLOCK (default: the latest) through the followed pools' logs, 100 blocks per
eth_getLogs, until it has picked, for v3, 5 blocks that mint or burn
liquidity at each tick spacing (1, 10, 60), a burn that empties a tick, and mints or burns
whose range starts at the price that change its liquidity on net, with no swap after them in
the block; and, for v2, 10 Syncs. It adds 2
blocks that only swap per spacing, from the newest 100 blocks. For each picked block N and pool
it writes the pool's state at N-1, the block's logs for it, and the state at N: v3 slot0,
liquidity and each tick a mint or burn touched; v2 getReserves.

The default RPC is dRPC's free Base endpoint: mainnet.base.org keeps every block's state too,
but answered eth_getLogs with 429 even for 10 blocks (2026-10-24).
"""
import argparse
import http.client
import json
import math
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor

SWAP = "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67"
MINT = "0x7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde"
BURN = "0x0c396cd989a39f4459b5fa1aed6a9a8dcdbc45908acfd67e028cd568da98982c"
INITIALIZE = "0x98636036cb66a9c19a37435efc1e90142190214e8abeb821bdba3f2990dd4c95"
SYNC = "0x1c411e9a96e071241c2f21f7726b17ae89e3cab4c78be50e062b03a9fffbbad1"
V3_TOPICS = [SWAP, MINT, BURN, INITIALIZE]

# Uniswap v3 pools on Base, from the factory's getPool: fee 100 (spacing 1), 500 (10), 3000 (60).
V3_POOLS = [
    "0xb4CB800910B228ED3d0834cF79D697127BBB00e5",  # WETH/USDC 100
    "0xE9e25E35aa99A2A60155010802b81A25C45bA185",  # USDC/cbBTC 100
    "0x06959273E9A65433De71F5A452D529544E07dDD0",  # USDbC/USDC 100
    "0xD56da2B74bA826f19015E6B7Dd9Dae1903E85DA1",  # USDC/USDT 100
    "0x4c1e404a013bF1917fAf2dBF5E5cEf3E9Bafb5aa",  # WETH/cbBTC 100
    "0xd0b53D9277642d899DF5C87A3966A349A798F224",  # WETH/USDC 500
    "0xd92E0767473D1E3FF11Ac036f2b1DB90aD0aE55F",  # 500, a pricing reference pool
    "0x6c561B446416E1A00E8E93E221854d6eA4171372",  # WETH/USDC 3000
    "0x8c7080564B5A792A33Ef2FD473fbA6364d5495e5",  # WETH/cbBTC 3000
    # Uniswap pools (factory() checked) where mints landed at the price often in Oct 2026.
    "0x1cca8388e671e83010843a1a0535b04f2d7e946b",  # spacing 60
    "0x2df380544b88adb3ad0a94100dcc45fd705aae2d",  # spacing 1
    "0xc18f50d6a832f12f6dcaaeee8d0c87a65b96787e",  # spacing 1
]
V2_PAIR = "0x88A43bbDF9D098eEC7bCEda4e2494615dfD9bB9C"  # WETH/USDC
SPACINGS = (1, 10, 60)
CHANGING, SWAP_ONLY, SYNCS = 5, 2, 10

parser = argparse.ArgumentParser()
parser.add_argument("--rpc", default="https://base.drpc.org")
parser.add_argument("--to", type=int)
parser.add_argument("--max-blocks", type=int, default=500_000)
args = parser.parse_args()
pool_of_threads = ThreadPoolExecutor(8)


def rpc(method, params):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    request = urllib.request.Request(args.rpc, body, {"content-type": "application/json", "user-agent": "omnimarket-capture"})
    for attempt in range(8):
        try:
            answer = json.load(urllib.request.urlopen(request, timeout=30))
        except (OSError, http.client.HTTPException, ValueError) as e:
            if attempt == 7:
                raise SystemExit(f"{method}: {e}")
            time.sleep(2**attempt)
            continue
        if "error" in answer:
            raise SystemExit(f"{method}: {answer['error']}")
        return answer["result"]


def call(to, data, block):
    return bytes.fromhex(rpc("eth_call", [{"to": to, "data": data}, hex(block)])[2:])


def calls(requests):
    """Several eth_calls at once: [(to, data, block)] → their answers, in order."""
    return list(pool_of_threads.map(lambda r: call(*r), requests))


def logs(address, topics, low, high):
    found = rpc("eth_getLogs", [{"fromBlock": hex(low), "toBlock": hex(high), "address": address, "topics": [topics]}])
    found = [l for l in found if not l.get("removed")]
    return sorted(found, key=lambda l: (int(l["blockNumber"], 16), int(l["logIndex"], 16)))


def word(data, i, signed=False):
    value = int.from_bytes(data[32 * i : 32 * i + 32], "big")
    return value - (1 << 256) if signed and value >> 255 else value


def topic_int(topic):
    value = int(topic, 16)
    return value - (1 << 256) if value >> 255 else value


def int24(value):
    """ABI-encoded: sign-extended to 32 bytes."""
    return (value % (1 << 256)).to_bytes(32, "big").hex()


def position(log):
    """A Mint's or Burn's (lower, upper, amount)."""
    data = bytes.fromhex(log["data"][2:])
    amount = word(data, 1) if log["topics"][0] == MINT else word(data, 0)
    return topic_int(log["topics"][2]), topic_int(log["topics"][3]), amount


def changes_liquidity(log):
    return log["topics"][0] in (MINT, BURN) and position(log)[2] > 0


def maybe_at_price(log):
    """Whether a mint or burn may have landed with the price in its lowest tick, from its own
    amounts: inside the range it moves token0, and amount1 = L·(√P − √P_lower)."""
    if not changes_liquidity(log):
        return False
    lower, _, amount = position(log)
    data = bytes.fromhex(log["data"][2:])
    first = 2 if log["topics"][0] == MINT else 1
    amount0, amount1 = word(data, first), word(data, first + 1)
    if amount0 == 0:
        return False
    sqrt_price = 1.0001 ** (lower / 2) + amount1 / amount
    return abs(math.log(sqrt_price**2, 1.0001) - lower) < 2


def may_start_at_price(pool, block, group):
    """`starts_at_price`, reading the pool's tick at N-1 only for a group that could pass."""
    last_swap = max((i for i, l in enumerate(group) if l["topics"][0] == SWAP), default=-1)
    if not any(maybe_at_price(l) for l in group[last_swap + 1 :]):
        return False
    tick_before = word(call(pool, "0x3850c7bd", block - 1), 1, signed=True)
    return starts_at_price(group, tick_before)


def touched(group):
    ticks = []
    for log in group:
        if log["topics"][0] in (MINT, BURN):
            ticks += [t for t in position(log)[:2] if t not in ticks]
    return ticks


def starts_at_price(group, tick_before):
    """Mints and burns of liquidity whose lower tick is the price when they land, changing the
    pool's liquidity on net (a mint and an equal burn cancel), with no swap after them to reset
    it."""
    current, net = tick_before, 0
    for log in group:
        if log["topics"][0] == SWAP:
            current, net = word(bytes.fromhex(log["data"][2:]), 4, signed=True), 0
        elif changes_liquidity(log) and position(log)[0] == current:
            net += position(log)[2] if log["topics"][0] == MINT else -position(log)[2]
    return net != 0


def v3_state(pool, block, ticks):
    """(sqrtPriceX96, tick, liquidity, {tick: (gross, net)}) at the end of `block`."""
    answers = calls([(pool, "0x3850c7bd", block), (pool, "0x1a686502", block)] + [(pool, "0xf30dba93" + int24(t), block) for t in ticks])
    slot0, liquidity, tick_answers = answers[0], answers[1], answers[2:]
    return (
        word(slot0, 0),
        word(slot0, 1, signed=True),
        word(liquidity, 0),
        {t: (word(a, 0), word(a, 1, signed=True)) for t, a in zip(ticks, tick_answers)},
    )


class V3Case:
    def __init__(self, pool, block):
        self.pool, self.block = pool, block
        self.logs = logs(pool, V3_TOPICS, block, block)
        ticks = touched(self.logs)
        self.before = v3_state(pool, block - 1, ticks)
        self.after = v3_state(pool, block, ticks)

    def empties_a_tick(self):
        burned = {t for l in self.logs if l["topics"][0] == BURN and changes_liquidity(l) for t in position(l)[:2]}
        return any(self.before[3][t][0] > 0 and self.after[3][t][0] == 0 for t in burned)

    def starts_at_price(self):
        return starts_at_price(self.logs, self.before[1])

    def lines(self):
        def state(s):
            return ["state {} {} {}".format(*s[:3])] + [f"tick {t} {g} {n}" for t, (g, n) in s[3].items()]

        return (
            [f"v3 {self.block} {self.pool} {spacing_of[self.pool]}"]
            + state(self.before)
            + [f"log {','.join(l['topics'])} {l['data']}" for l in self.logs]
            + state(self.after)
        )


def v2_lines(block, syncs):
    before, after = calls([(V2_PAIR, "0x0902f1ac", block - 1), (V2_PAIR, "0x0902f1ac", block)])
    return (
        [f"v2 {block} {V2_PAIR}", f"reserves {word(before, 0)} {word(before, 1)}"]
        + [f"log {','.join(l['topics'])} {l['data']}" for l in syncs]
        + [f"reserves {word(after, 0)} {word(after, 1)}"]
    )


latest = int(rpc("eth_blockNumber", []), 16)
spacing_of = dict(zip(V3_POOLS, (word(a, 0, signed=True) for a in calls([(p, "0xd0c93a7c", latest) for p in V3_POOLS]))))
end = args.to or latest
picked = {}  # (block, pool) → lines
changing = {s: 0 for s in SPACINGS}
need = {"empties a tick": True, "starts at the price": True}
syncs = 0

# Blocks that only swap, from the newest 100 blocks.
swap_only = {s: 0 for s in SPACINGS}
groups = {}
for log in logs(V3_POOLS, V3_TOPICS, end - 99, end):
    groups.setdefault((int(log["blockNumber"], 16), log["address"]), []).append(log)
for (block, pool), group in sorted(groups.items(), key=lambda kv: (-kv[0][0], kv[0][1])):
    address = next(p for p in V3_POOLS if p.lower() == pool.lower())
    s = spacing_of[address]
    if s in SPACINGS and swap_only[s] < SWAP_ONLY and all(l["topics"][0] == SWAP for l in group):
        picked[(block, address)] = V3Case(address, block).lines()
        swap_only[s] += 1

high = end
while end - high < args.max_blocks:
    if all(changing[s] >= CHANGING for s in SPACINGS) and not any(need.values()) and syncs >= SYNCS:
        break
    low = high - 99
    groups = {}
    for log in logs(V3_POOLS + [V2_PAIR], V3_TOPICS + [SYNC], low, high):
        groups.setdefault((int(log["blockNumber"], 16), log["address"]), []).append(log)
    for (block, pool), group in sorted(groups.items(), key=lambda kv: (-kv[0][0], kv[0][1])):
        if pool.lower() == V2_PAIR.lower():
            if syncs < SYNCS:
                picked[(block, V2_PAIR)] = v2_lines(block, group)
                syncs += len(group)
            continue
        address = next(p for p in V3_POOLS if p.lower() == pool.lower())
        s = spacing_of[address]
        if s not in SPACINGS or not any(changes_liquidity(l) for l in group) or (block, address) in picked:
            continue
        if changing[s] >= CHANGING and not (need["empties a tick"] or (need["starts at the price"] and may_start_at_price(address, block, group))):
            continue
        case = V3Case(address, block)
        special = [k for k, test in (("empties a tick", case.empties_a_tick), ("starts at the price", case.starts_at_price)) if need[k] and test()]
        if changing[s] < CHANGING or special:
            picked[(block, address)] = case.lines()
            changing[s] += 1
            for k in special:
                need[k] = False
    print(f"{low}..{high}: changing {changing}, swap only {swap_only}, need {[k for k, v in need.items() if v]}, syncs {syncs}", flush=True)
    high = low - 1
else:
    raise SystemExit(f"stopped after {args.max_blocks} blocks without every case")

out = __file__.rsplit("/", 1)[0] + "/base-blocks.txt"
with open(out, "w") as f:
    f.write(f"# capture.py --to {end}: {len(picked)} blocks from {high + 1}..{end}\n")
    for key in sorted(picked):
        f.write("\n".join(picked[key]) + "\n")
print(f"wrote {len(picked)} cases to {out}")
