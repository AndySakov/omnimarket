#!/usr/bin/env python3
"""Captures Base's reference pools at one block into reference-pools.txt (#76).

    crates/pricing/tests/fixtures/capture.py [RPC]   (default: base-rpc.publicnode.com)

Reads each Uniswap v3 pool's slot0, liquidity and the initialized ticks in the TickLens words
within ±3% of its price, and the v2 pair's reserves, all at the latest block. PublicNode keeps
about 90 blocks of state (D82), so a capture is one run, not something to resume.
"""
import json
import sys
import urllib.request

RPC = sys.argv[1] if len(sys.argv) > 1 else "https://base-rpc.publicnode.com"
TICK_LENS = "0x0CdeE061c75D43c82520eD998C23ac2991c9ac6d"
V3_POOLS = [  # address, fee, tick spacing: PricingConfig::base().reference_pools
    ("0xd0b53D9277642d899DF5C87A3966A349A798F224", 500, 10),
    ("0x6c561B446416E1A00E8E93E221854d6eA4171372", 3000, 60),
    ("0xd92E0767473D1E3FF11Ac036f2b1DB90aD0aE55F", 500, 10),
]
V2_PAIR = "0x88A43bbDF9D098eEC7bCEda4e2494615dfD9bB9C"  # WETH/USDC, checked against CREATE2 in the test


def rpc(method, params):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    request = urllib.request.Request(RPC, body, {"content-type": "application/json", "user-agent": "omnimarket-capture"})
    answer = json.load(urllib.request.urlopen(request))
    if "error" in answer:
        raise SystemExit(f"{method}: {answer['error']}")
    return answer["result"]


def call(to, data, block):
    return bytes.fromhex(rpc("eth_call", [{"to": to, "data": data}, block])[2:])


def word(data, i, signed=False):
    value = int.from_bytes(data[32 * i : 32 * i + 32], "big")
    return value - (1 << 256) if signed and value >> 255 else value


def int16(value):
    """ABI-encoded: sign-extended to 32 bytes."""
    return (value % (1 << 256)).to_bytes(32, "big").hex()


block = rpc("eth_blockNumber", [])
lines = [f"block {int(block, 16)}"]
for pool, fee, spacing in V3_POOLS:
    slot0 = call(pool, "0x3850c7bd", block)
    sqrt_price, tick = word(slot0, 0), word(slot0, 1, signed=True)
    liquidity = word(call(pool, "0x1a686502", block), 0)
    lines.append(f"v3 {pool} {fee} {spacing} {sqrt_price} {tick} {liquidity}")
    reach = 300 // spacing + 1  # ±3% is about ±296 ticks
    first, last = ((tick // spacing - reach) >> 8), ((tick // spacing + reach) >> 8)
    for w in range(first, last + 1):
        data = "0x351fb478" + pool[2:].lower().rjust(64, "0") + int16(w)
        out = call(TICK_LENS, data, block)
        count = word(out, 1)
        for i in range(count):
            t = word(out, 2 + 3 * i, signed=True)
            net = word(out, 3 + 3 * i, signed=True)
            gross = word(out, 4 + 3 * i)
            lines.append(f"tick {pool} {t} {gross} {net}")
reserves = call(V2_PAIR, "0x0902f1ac", block)
lines.append(f"v2 {V2_PAIR} {word(reserves, 0)} {word(reserves, 1)}")
out = __file__.rsplit("/", 1)[0] + "/reference-pools.txt"
with open(out, "w") as f:
    f.write("\n".join(lines) + "\n")
print(f"wrote {len(lines)} lines at block {int(block, 16)} to {out}")
