// SPDX-License-Identifier: MIT
pragma solidity 0.8.35;

/// @title IArbSys
/// @notice Arbitrum system precompile functions used by bridge contracts.
interface IArbSys {
    /// @notice Returns the current Arbitrum child-chain block number.
    /// @dev Solidity's `block.number` on Arbitrum exposes an approximate L1
    ///      block number, so it must not be used for RGB lock-height checks.
    function arbBlockNumber() external view returns (uint256);
}
