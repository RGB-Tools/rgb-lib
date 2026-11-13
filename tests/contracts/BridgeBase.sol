// SPDX-License-Identifier: MIT
pragma solidity 0.8.35;

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {SafeERC20, IERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Pausable} from "@openzeppelin/contracts/utils/Pausable.sol";

/// @title BridgeBase
/// @notice Abstract base contract shared by MinimalBridge and Bridge.
///
/// @dev Provides:
///      - Single accepted token (immutable, set at deploy).
///      - pause / unpause (owner-only).
///      - Permanently blocked renounceOwnership.
///      - Utility views: getChainId, getContractBalance.
abstract contract BridgeBase is Ownable2Step, Pausable {
    using SafeERC20 for IERC20;

    // =========================================================================
    // State
    // =========================================================================

    /// @notice The only accepted ERC-20 token. Immutable after deploy.
    address public immutable TOKEN;

    /// @notice Outflow (withdrawal) freeze flag, independent of the inflow
    ///         freeze. Inflow reuses OpenZeppelin `Pausable._paused` (gated by
    ///         `whenNotPaused` on `fundsIn`); this second flag gates `fundsOut`
    ///         via `whenOutflowNotPaused`. Two independent flags are required so
    ///         that a planned inflow-only pause (deposits frozen, withdrawals
    ///         still open for liquidity migration) and an emergency pause (both
    ///         frozen) can be expressed separately.
    bool private _outflowPaused;

    // =========================================================================
    // Events
    // =========================================================================

    /// @dev The `FundsIn` event is declared by each concrete bridge, because
    ///      the two variants use different `operationId` types: the production
    ///      `Bridge` derives a `bytes32` id on-chain (see `IBridge`), while the
    ///      `MinimalBridge` echoes a caller-supplied `uint256` id.

    /// @notice Emitted when the outflow (withdrawal) path is frozen.
    /// @dev The inflow path reuses OpenZeppelin `Pausable`, which emits its own
    ///      `Paused` / `Unpaused` events.
    event OutflowPaused(address account);

    /// @notice Emitted when the outflow (withdrawal) path is unfrozen.
    event OutflowUnpaused(address account);

    // =========================================================================
    // Errors
    // =========================================================================

    error InvalidTokenAddress();
    error InvalidRecipientAddress();
    error AmountExceedBridgePool();
    error AmountExceedsUint64(uint256 amount);
    error RenounceOwnershipBlocked();

    /// @notice Thrown by `whenOutflowNotPaused` when the outflow path is frozen.
    error OutflowEnforcedPause();

    // =========================================================================
    // Modifiers
    // =========================================================================

    /// @dev Reverts when the outflow path is frozen. Mirrors OpenZeppelin's
    ///      `whenNotPaused` (which gates inflow) but for the independent
    ///      `_outflowPaused` flag, so `fundsOut` can be frozen without freezing
    ///      `fundsIn` and vice versa.
    modifier whenOutflowNotPaused() {
        if (_outflowPaused) revert OutflowEnforcedPause();
        _;
    }

    // =========================================================================
    // Constructor
    // =========================================================================

    constructor(address token_) Ownable(msg.sender) {
        if (token_ == address(0)) revert InvalidTokenAddress();
        TOKEN = token_;
    }

    // =========================================================================
    // Owner-only
    // =========================================================================

    /// @notice Freeze the inflow (deposit) path only; withdrawals stay open.
    /// @dev Intended for planned operations such as a bridge upgrade or
    ///      liquidity migration. On the production `Bridge` this is reached
    ///      through the timelocked `MultisigProxy` propose -> execute path
    ///      (`PauseInflow` operation), giving the federation an observation
    ///      window before it takes effect.
    function pauseInflow() external onlyOwner {
        _pause();
    }

    /// @notice Resume the inflow (deposit) path.
    function unpauseInflow() external onlyOwner {
        _unpause();
    }

    /// @notice Emergency freeze of BOTH inflow and outflow, set atomically.
    /// @dev No-timelock control for incident response. On the production
    ///      `Bridge` it is reached through the federation-signed
    ///      `MultisigProxy.emergencyPause` (federation signatures, no propose
    ///      step) or its configured emergency guardian path.
    ///      Each flag is set idempotently, so the call does not revert if one
    ///      side is already frozen (e.g. inflow already paused via the planned
    ///      path). Freezing `fundsOut` also freezes the enclave/TEE release
    ///      path, which routes through `fundsOut`.
    function emergencyPauseAll() external onlyOwner {
        if (!paused()) _pause(); // inflow (OpenZeppelin flag), guarded against double-pause
        if (!_outflowPaused) {
            _outflowPaused = true;
            emit OutflowPaused(_msgSender());
        }
    }

    /// @notice Lift the emergency freeze on BOTH inflow and outflow. Idempotent.
    function emergencyUnpauseAll() external onlyOwner {
        if (paused()) _unpause();
        if (_outflowPaused) {
            _outflowPaused = false;
            emit OutflowUnpaused(_msgSender());
        }
    }

    /// @notice Permanently blocks renouncing ownership.
    function renounceOwnership() public view virtual override onlyOwner {
        revert RenounceOwnershipBlocked();
    }

    // =========================================================================
    // View
    // =========================================================================

    /// @notice Returns the token balance held by the contract (bridgeable liquidity).
    function getContractBalance() external view returns (uint256) {
        return IERC20(TOKEN).balanceOf(address(this));
    }

    /// @notice Whether the outflow (withdrawal) path is currently frozen.
    /// @dev The inflow freeze is exposed by OpenZeppelin's `paused()`.
    function outflowPaused() external view returns (bool) {
        return _outflowPaused;
    }

    /// @notice Returns the current chain ID.
    function getChainId() public view returns (uint256) {
        uint256 id;
        assembly { id := chainid() }
        return id;
    }
}
