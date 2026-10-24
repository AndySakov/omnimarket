// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

interface IERC20 {
    function balanceOf(address) external view returns (uint256);
    function transfer(address, uint256) external returns (bool);
}

interface IWETH is IERC20 {
    function deposit() external payable;
}

interface IUniswapV2Pair {
    function token0() external view returns (address);
    function getReserves() external view returns (uint112, uint112, uint32);
    function swap(uint256 amount0Out, uint256 amount1Out, address to, bytes calldata data) external;
}

interface IUniswapV3Pool {
    function token0() external view returns (address);
    function swap(address recipient, bool zeroForOne, int256 amountSpecified, uint160 sqrtPriceLimitX96, bytes calldata data)
        external
        returns (int256 amount0, int256 amount1);
}

/// D29's round trip (#89): buys `token` with WETH against its live pool, then sells all of it back, and
/// reports what each leg lost to the token's own transfer rules. It's never deployed: an eth_call injects
/// its code at a throwaway address holding ETH, so it holds nothing and approves nothing outside that call.
contract SafetyProbe {
    struct Result {
        bool bought;
        bool sold;
        // The pool's own output for the buy, and what arrived.
        uint256 expectedTokens;
        uint256 receivedTokens;
        // What was sent to sell, and what reached the pool.
        uint256 tokensSold;
        uint256 poolReceived;
        // WETH back from the sell.
        uint256 returned;
        // The revert reason of the leg that failed.
        string reason;
    }

    uint160 internal constant MIN_SQRT = 4295128740;
    uint160 internal constant MAX_SQRT = 1461446703485210103287273052203988822378723970341;

    /// The v3 pool a swap is under way with: only it may call the swap callback.
    address internal active;

    receive() external payable {}

    function probe(address weth, address token, address pool, bool v3, uint256 amountIn)
        external
        returns (Result memory r)
    {
        IWETH(weth).deposit{value: amountIn}();
        try this.buy(weth, token, pool, v3, amountIn) returns (uint256 expected, uint256 received) {
            r.bought = true;
            r.expectedTokens = expected;
            r.receivedTokens = received;
        } catch (bytes memory why) {
            r.reason = reasonOf(why);
            return r;
        }
        r.tokensSold = r.receivedTokens;
        try this.sell(weth, token, pool, v3, r.tokensSold) returns (uint256 poolReceived, uint256 returned) {
            r.sold = true;
            r.poolReceived = poolReceived;
            r.returned = returned;
        } catch (bytes memory why) {
            r.reason = reasonOf(why);
        }
    }

    /// The buy leg; external so `probe` can catch its revert, the token's balance read included.
    /// Returns the tokens the pool sent, and what arrived.
    function buy(address weth, address token, address pool, bool v3, uint256 amountIn) external returns (uint256, uint256) {
        require(msg.sender == address(this), "only the probe");
        uint256 sent = v3 ? buyV3(weth, pool, amountIn) : buyV2(weth, pool, amountIn);
        return (sent, IERC20(token).balanceOf(address(this)));
    }

    function buyV3(address weth, address pool, uint256 amountIn) internal returns (uint256) {
        bool wethIs0 = IUniswapV3Pool(pool).token0() == weth;
        active = pool;
        (int256 a0, int256 a1) =
            IUniswapV3Pool(pool).swap(address(this), wethIs0, int256(amountIn), wethIs0 ? MIN_SQRT : MAX_SQRT, abi.encode(weth));
        active = address(0);
        return uint256(-(wethIs0 ? a1 : a0));
    }

    function buyV2(address weth, address pool, uint256 amountIn) internal returns (uint256) {
        (uint256 rWeth, uint256 rToken, bool wethIs0) = reserves(IUniswapV2Pair(pool), weth);
        uint256 out = amountOut(amountIn, rWeth, rToken);
        require(IERC20(weth).transfer(pool, amountIn), "weth transfer");
        if (wethIs0) IUniswapV2Pair(pool).swap(0, out, address(this), "");
        else IUniswapV2Pair(pool).swap(out, 0, address(this), "");
        return out;
    }

    /// The sell leg: everything bought, back into WETH. Returns what reached the pool and the WETH out.
    function sell(address weth, address token, address pool, bool v3, uint256 amount) external returns (uint256, uint256) {
        require(msg.sender == address(this), "only the probe");
        return v3 ? sellV3(token, pool, amount) : sellV2(weth, token, pool, amount);
    }

    function sellV3(address token, address pool, uint256 amount) internal returns (uint256, uint256) {
        uint256 before = IERC20(token).balanceOf(pool);
        bool tokenIs0 = IUniswapV3Pool(pool).token0() == token;
        active = pool;
        (int256 a0, int256 a1) =
            IUniswapV3Pool(pool).swap(address(this), tokenIs0, int256(amount), tokenIs0 ? MIN_SQRT : MAX_SQRT, abi.encode(token));
        active = address(0);
        return (IERC20(token).balanceOf(pool) - before, uint256(-(tokenIs0 ? a1 : a0)));
    }

    function sellV2(address weth, address token, address pool, uint256 amount) internal returns (uint256, uint256) {
        uint256 before = IERC20(token).balanceOf(pool);
        (uint256 rWeth, uint256 rToken, bool wethIs0) = reserves(IUniswapV2Pair(pool), weth);
        require(IERC20(token).transfer(pool, amount), "token transfer");
        uint256 got = IERC20(token).balanceOf(pool) - before;
        uint256 out = amountOut(got, rToken, rWeth);
        if (wethIs0) IUniswapV2Pair(pool).swap(out, 0, address(this), "");
        else IUniswapV2Pair(pool).swap(0, out, address(this), "");
        return (got, out);
    }

    /// Pays a v3 pool what the swap owes it, in the token `data` names.
    function uniswapV3SwapCallback(int256 amount0, int256 amount1, bytes calldata data) external {
        require(msg.sender == active, "not the pool");
        address paying = abi.decode(data, (address));
        uint256 owed = uint256(amount0 > 0 ? amount0 : amount1);
        require(IERC20(paying).transfer(msg.sender, owed), "payment");
    }

    function reserves(IUniswapV2Pair pair, address weth) internal view returns (uint256 rWeth, uint256 rToken, bool wethIs0) {
        (uint112 r0, uint112 r1,) = pair.getReserves();
        wethIs0 = pair.token0() == weth;
        (rWeth, rToken) = wethIs0 ? (uint256(r0), uint256(r1)) : (uint256(r1), uint256(r0));
    }

    /// UniswapV2Library.getAmountOut.
    function amountOut(uint256 amountIn, uint256 reserveIn, uint256 reserveOut) internal pure returns (uint256) {
        uint256 withFee = amountIn * 997;
        return withFee * reserveOut / (reserveIn * 1000 + withFee);
    }

    /// An `Error(string)` revert's message, or the raw data in hex. Data that only looks like an
    /// `Error(string)` is shown in hex, never decoded, so it can't revert the probe.
    function reasonOf(bytes memory why) internal pure returns (string memory) {
        if (why.length >= 68 && bytes4(why) == 0x08c379a0) {
            uint256 offset;
            uint256 length;
            assembly {
                offset := mload(add(why, 36))
                length := mload(add(why, 68))
            }
            if (offset == 32 && length <= why.length - 68) {
                bytes memory text = new bytes(length);
                for (uint256 i = 0; i < length; i++) {
                    text[i] = why[68 + i];
                }
                return string(text);
            }
        }
        bytes memory digits = "0123456789abcdef";
        bytes memory text = new bytes(2 + why.length * 2);
        text[0] = "0";
        text[1] = "x";
        for (uint256 i = 0; i < why.length; i++) {
            text[2 + 2 * i] = digits[uint8(why[i]) >> 4];
            text[3 + 2 * i] = digits[uint8(why[i]) & 0x0f];
        }
        return string(text);
    }
}
