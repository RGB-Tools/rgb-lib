// SPDX-License-Identifier: MIT
pragma solidity 0.8.35;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";

import {BridgeBase} from "./BridgeBase.sol";

/// @title MinimalBridge
/// @notice Minimal single-token bridge for lock/unlock operations.
///
/// @dev - No TEE signature verification.
///      - `fundsOut` is owner-only; the owner is expected to be a multisig or
///        similar access control contract on the integrator's side.
contract MinimalBridge is BridgeBase {
    using SafeERC20 for IERC20;

    // =========================================================================
    // Events
    // =========================================================================

    /// @notice Emitted on every fundsIn.
    /// @param sender      Address that deposited the tokens.
    /// @param operationId Backend-assigned operation identifier (the RGB OpId).
    /// @param amount      Amount of tokens locked.
    event FundsIn(address indexed sender, uint256 indexed operationId, uint64 amount);

    /// @notice Emitted when tokens are released from the bridge.
    /// @param recipient       Recipient on this chain.
    /// @param amount          Amount of tokens released.
    /// @param operationId     Backend-assigned operation identifier.
    /// @param sourceAddress   Sender address on the source chain (e.g. RGB address).
    event FundsOut(address indexed recipient, uint256 amount, uint256 indexed operationId, string sourceAddress);

    // =========================================================================
    // Constructor
    // =========================================================================

    /// @param token_ ERC-20 token address accepted by this bridge (e.g. USDT0).
    constructor(address token_) BridgeBase(token_) {}

    // =========================================================================
    // External — user-facing
    // =========================================================================

    /// @notice Lock tokens in the bridge to initiate a transfer to the destination chain.
    /// @param amount      Amount of tokens to lock.
    /// @param operationId Backend-assigned operation identifier included in the event.
    function fundsIn(uint256 amount, uint256 operationId) external whenNotPaused {
        if (amount > type(uint64).max) revert AmountExceedsUint64(amount);

        IERC20(TOKEN).safeTransferFrom(msg.sender, address(this), amount);

        emit FundsIn(msg.sender, operationId, uint64(amount));
    }

    // =========================================================================
    // External — owner-only
    // =========================================================================

    /// @notice Release tokens from the bridge to a recipient. Only callable by owner.
    /// @param recipient     Recipient address on this chain.
    /// @param amount        Amount of tokens to release.
    /// @param operationId   Backend-assigned operation identifier included in the event.
    /// @param sourceAddress Sender address on the source chain (e.g. RGB address).
    function fundsOut(address recipient, uint256 amount, uint256 operationId, string calldata sourceAddress)
        external
        onlyOwner
        whenOutflowNotPaused
    {
        if (recipient == address(0)) revert InvalidRecipientAddress();
        if (amount > IERC20(TOKEN).balanceOf(address(this))) revert AmountExceedBridgePool();

        IERC20(TOKEN).safeTransfer(recipient, amount);

        emit FundsOut(recipient, amount, operationId, sourceAddress);
    }
}
