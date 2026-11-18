// SPDX-License-Identifier: MIT
pragma solidity 0.8.35;

import {IArbSys} from "./interfaces/IArbSys.sol";

/// @title MockArbSys
/// @notice Stand-in for the Arbitrum system precompile on a local chain, where
///         its code is installed at the precompile address (`address(100)`).
///         Locally the child-chain block number is the block number itself.
contract MockArbSys is IArbSys {
    function arbBlockNumber() external view returns (uint256) {
        return block.number;
    }
}
