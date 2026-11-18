// SPDX-License-Identifier: MIT
pragma solidity 0.8.35;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";

import {BridgeBase} from "./BridgeBase.sol";
import {IArbSys} from "./interfaces/IArbSys.sol";

/// @title MinimalBridge
/// @notice Minimal single-token bridge for lock/unlock operations.
///
/// @dev - No TEE signature verification.
///      - `fundsOut` is owner-only; the owner is expected to be a multisig or
///        similar access control contract on the integrator's side.
contract MinimalBridge is BridgeBase {
    using SafeERC20 for IERC20;

    // =========================================================================
    // Constants
    // =========================================================================

    /// @notice Arbitrum system precompile exposing the child-chain block number.
    IArbSys internal constant ARBSYS = IArbSys(address(100));

    /// @notice Domain-separated type hash for permanent RGB mint records.
    bytes32 public constant RGB_MINT_DEPOSIT_TYPEHASH =
        keccak256("UtexoRgbMintDeposit(address bridge,uint256 chainId,address token,uint256 rgbLockId,uint256 amount)");

    // =========================================================================
    // Storage
    // =========================================================================

    /// @notice Permanent funded RGB mint heights, keyed by the complete claim.
    /// @dev Zero means absent; otherwise the value is the actual L2 block plus one.
    mapping(bytes32 claimKey => uint64 l2BlockPlusOne) public fundsInRecords;

    /// @notice Current amount of RGB units backed by this bridge.
    /// @dev Increases on successful fundsIn and decreases on successful fundsOut.
    ///      Direct token transfers do not affect this accounted supply.
    uint64 public circulatingSupply;

    // =========================================================================
    // Events
    // =========================================================================

    /// @notice Emitted on every fundsIn.
    /// @param sender                 Address that deposited the tokens.
    /// @param lockId                 RGB lock identifier, committing to the mint operation and
    ///                               its witness transaction.
    /// @param amount                 Amount of tokens locked.
    event FundsIn(address indexed sender, uint256 indexed lockId, uint64 amount);

    /// @notice Emitted when tokens are released from the bridge.
    /// @param recipient              Recipient on this chain.
    /// @param amount                 Amount of tokens released.
    /// @param operationId            Backend-assigned operation identifier.
    /// @param sourceAddress          Sender address on the source chain (e.g. RGB address).
    event FundsOut(address indexed recipient, uint256 amount, uint256 indexed operationId, string sourceAddress);

    // =========================================================================
    // Errors
    // =========================================================================

    /// @notice The RGB lock identifier is zero.
    error InvalidLockId();

    /// @notice A record already exists for the complete RGB mint claim.
    error DuplicateMintDeposit(bytes32 claimKey);

    /// @notice The lock was submitted at or after its exclusive maximum L2 block.
    error LockWindowExpired(uint256 l2Block, uint64 beforeBlock);

    /// @notice A deposit would make the RGB circulating supply exceed uint64.
    error CirculatingSupplyOverflow(uint64 currentSupply, uint256 amount);

    /// @notice A release exceeds the accounted RGB circulating supply.
    error AmountExceedsCirculatingSupply(uint256 amount, uint64 currentSupply);

    // =========================================================================
    // Constructor
    // =========================================================================

    /// @param token_                 ERC-20 token address accepted by this bridge (e.g. USDT0).
    constructor(address token_) BridgeBase(token_) {}

    // =========================================================================
    // External — user-facing
    // =========================================================================

    /// @notice Lock tokens in the bridge to initiate a transfer to the destination chain.
    /// @param amount                 Amount of tokens to lock.
    /// @param lockId                 RGB lock identifier (tagged SHA-256 of the mint operation ID
    ///                               and of its witness TXID), included in the event.
    /// @param beforeBlock             Exclusive maximum Arbitrum L2 block for the lock.
    function fundsIn(uint256 amount, uint256 lockId, uint64 beforeBlock) external whenNotPaused {
        if (amount > type(uint64).max) revert AmountExceedsUint64(amount);
        if (lockId == 0) revert InvalidLockId();

        uint256 l2Block = ARBSYS.arbBlockNumber();
        if (l2Block >= beforeBlock) revert LockWindowExpired(l2Block, beforeBlock);

        bytes32 claimKey = rgbMintDepositId(lockId, amount);
        if (fundsInRecords[claimKey] != 0) revert DuplicateMintDeposit(claimKey);

        uint256 newSupply = uint256(circulatingSupply) + amount;
        if (newSupply > type(uint64).max) revert CirculatingSupplyOverflow(circulatingSupply, amount);

        // Safe after the explicit block-number bound above.
        // forge-lint: disable-next-line(unsafe-typecast)
        uint64 recordedBlockPlusOne = uint64(l2Block + 1);

        // Write before the token interaction so the same claim cannot be
        // re-entered and supply remains consistent during the interaction.
        // A transfer failure reverts both writes atomically.
        fundsInRecords[claimKey] = recordedBlockPlusOne;
        // Safe after the explicit uint64 bound above.
        // forge-lint: disable-next-line(unsafe-typecast)
        circulatingSupply = uint64(newSupply);

        IERC20(TOKEN).safeTransferFrom(msg.sender, address(this), amount);

        // forge-lint: disable-next-line(unsafe-typecast)
        emit FundsIn(msg.sender, lockId, uint64(amount));
    }

    // =========================================================================
    // Views
    // =========================================================================

    /// @notice Derives the permanent record key for an RGB mint claim.
    function rgbMintDepositId(uint256 lockId, uint256 amount) public view returns (bytes32) {
        return
            keccak256(abi.encode(RGB_MINT_DEPOSIT_TYPEHASH, address(this), block.chainid, TOKEN, lockId, amount));
    }

    /// @notice Returns the funded L2 height for one RGB mint claim.
    /// @dev Zero means absent; otherwise subtract one to recover the L2 block.
    function getFundsInBlock(uint256 lockId, uint256 amount) external view returns (uint64 l2BlockPlusOne) {
        return fundsInRecords[rgbMintDepositId(lockId, amount)];
    }

    // =========================================================================
    // External — owner-only
    // =========================================================================

    /// @notice Release tokens from the bridge to a recipient. Only callable by owner.
    /// @param recipient              Recipient address on this chain.
    /// @param amount                 Amount of tokens to release.
    /// @param operationId            Backend-assigned operation identifier included in the event.
    /// @param sourceAddress          Sender address on the source chain (e.g. RGB address).
    function fundsOut(address recipient, uint256 amount, uint256 operationId, string calldata sourceAddress)
        external
        onlyOwner
        whenOutflowNotPaused
    {
        if (recipient == address(0)) revert InvalidRecipientAddress();
        if (amount > IERC20(TOKEN).balanceOf(address(this))) revert AmountExceedBridgePool();
        uint64 currentSupply = circulatingSupply;
        if (amount > currentSupply) revert AmountExceedsCirculatingSupply(amount, currentSupply);

        // Safe because amount cannot exceed the uint64 supply.
        // forge-lint: disable-next-line(unsafe-typecast)
        circulatingSupply = currentSupply - uint64(amount);

        IERC20(TOKEN).safeTransfer(recipient, amount);

        emit FundsOut(recipient, amount, operationId, sourceAddress);
    }
}
