// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {SafetyProbe} from "../src/SafetyProbe.sol";

/// A plain token; `cut` takes a share of every transfer, and `honeypot` refuses transfers to `pair` from
/// anyone but the owner, so buys work and sells don't.
contract Token {
    mapping(address => uint256) public balanceOf;
    address public immutable owner = msg.sender;
    address public pair;
    uint256 public immutable cutPpm;
    bool public immutable honeypot;

    constructor(uint256 cutPpm_, bool honeypot_) {
        cutPpm = cutPpm_;
        honeypot = honeypot_;
    }

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function setPair(address pair_) external {
        pair = pair_;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        require(!(honeypot && to == pair && msg.sender != owner), "sells are disabled");
        balanceOf[msg.sender] -= amount;
        uint256 cut = amount * cutPpm / 1_000_000;
        balanceOf[to] += amount - cut;
        balanceOf[address(0xdead)] += cut;
        return true;
    }
}

contract WETH {
    mapping(address => uint256) public balanceOf;

    function deposit() external payable {
        balanceOf[msg.sender] += msg.value;
    }

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

interface IBalance {
    function balanceOf(address) external view returns (uint256);
    function transfer(address, uint256) external returns (bool);
}

/// Uniswap v2's pair, cut to what the probe calls: reserves follow balances, and a swap keeps k after
/// the 0.3% fee.
contract Pair {
    address public immutable token0;
    address public immutable token1;
    uint112 internal reserve0;
    uint112 internal reserve1;

    constructor(address a, address b) {
        (token0, token1) = a < b ? (a, b) : (b, a);
    }

    function sync() public {
        reserve0 = uint112(IBalance(token0).balanceOf(address(this)));
        reserve1 = uint112(IBalance(token1).balanceOf(address(this)));
    }

    function getReserves() external view returns (uint112, uint112, uint32) {
        return (reserve0, reserve1, 0);
    }

    function swap(uint256 out0, uint256 out1, address to, bytes calldata) external {
        if (out0 > 0) require(IBalance(token0).transfer(to, out0), "out0");
        if (out1 > 0) require(IBalance(token1).transfer(to, out1), "out1");
        uint256 b0 = IBalance(token0).balanceOf(address(this));
        uint256 b1 = IBalance(token1).balanceOf(address(this));
        uint256 in0 = b0 > reserve0 - out0 ? b0 - (reserve0 - out0) : 0;
        uint256 in1 = b1 > reserve1 - out1 ? b1 - (reserve1 - out1) : 0;
        require((b0 * 1000 - in0 * 3) * (b1 * 1000 - in1 * 3) >= uint256(reserve0) * reserve1 * 1e6, "K");
        sync();
    }
}

contract SafetyProbeTest {
    SafetyProbe internal probe;
    WETH internal weth;

    function setUp() public {
        weth = new WETH();
        probe = new SafetyProbe();
        payable(address(probe)).transfer(1 ether);
    }

    /// A pool of 1,000 WETH against 1,000,000 of `token`, and the probe's round trip of 0.1 WETH.
    function roundTrip(uint256 cutPpm, bool honeypot) internal returns (SafetyProbe.Result memory) {
        Token token = new Token(cutPpm, honeypot);
        Pair pair = new Pair(address(weth), address(token));
        token.setPair(address(pair));
        weth.mint(address(pair), 1_000 ether);
        token.mint(address(pair), 1_000_000 ether);
        pair.sync();
        return probe.probe(address(weth), address(token), address(pair), false, 0.1 ether);
    }

    /// 1 - got / expected, in parts per million.
    function lossPpm(uint256 got, uint256 expected) internal pure returns (uint256) {
        return (expected - got) * 1_000_000 / expected;
    }

    function test_a_plain_token_sells_back_with_no_tax() public {
        SafetyProbe.Result memory r = roundTrip(0, false);
        require(r.bought && r.sold, "round trip");
        require(r.receivedTokens == r.expectedTokens, "no buy tax");
        require(r.poolReceived == r.tokensSold, "no sell tax");
        // Two 0.3% pool fees, and nothing else.
        require(lossPpm(r.returned, 0.1 ether) < 6_100, "returned");
    }

    function test_a_taxed_token_reads_its_tax_within_a_tenth_of_a_percent() public {
        SafetyProbe.Result memory r = roundTrip(50_000, false);
        require(r.bought && r.sold, "round trip");
        uint256 buyTax = lossPpm(r.receivedTokens, r.expectedTokens);
        uint256 sellTax = lossPpm(r.poolReceived, r.tokensSold);
        require(buyTax >= 49_000 && buyTax <= 51_000, "buy tax 5%");
        require(sellTax >= 49_000 && sellTax <= 51_000, "sell tax 5%");
    }

    function test_a_honeypot_buys_but_wont_sell() public {
        SafetyProbe.Result memory r = roundTrip(0, true);
        require(r.bought, "bought");
        require(!r.sold, "a honeypot doesn't sell");
        require(r.returned == 0, "nothing back");
        require(keccak256(bytes(r.reason)) == keccak256("sells are disabled"), r.reason);
    }

    function test_only_the_probe_runs_a_leg() public {
        try probe.buy(address(weth), address(0), false, 1) {
            revert("an outside caller ran a leg");
        } catch Error(string memory reason) {
            require(keccak256(bytes(reason)) == keccak256("only the probe"), reason);
        }
    }
}
