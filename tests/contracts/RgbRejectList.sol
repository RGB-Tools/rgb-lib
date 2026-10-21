// SPDX-License-Identifier: MIT
pragma solidity 0.8.35;

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";

import {IRgbRejectList} from "./interfaces/IRgbRejectList.sol";

/// @title RgbRejectList
/// @notice Append-only on-chain reject list for RGB client-side validation.
///         See `IRgbRejectList` for the read model clients are expected to use.
///
/// @dev    Standalone: no bridge contract reads or writes it. It is not
///         upgradeable.
///
///         There is deliberately no function that edits or removes an entry,
///         so append-only is a property of the contract rather than an
///         operational convention — the incremental client sync relies on it.
///         A decision is changed by appending a newer entry for the same
///         opout; the latest entry wins.
///
///         Two roles. The owner — meant to be a cold key — only appoints and
///         rotates the appender; the appender — the hot key of the publishing
///         tool — is the only account that can append. Neither can be the zero
///         address. Ownership uses a two-step transfer and cannot be renounced:
///         the registry cannot be replaced, so losing the owner would freeze
///         the list permanently.
contract RgbRejectList is IRgbRejectList, Ownable2Step {
    // =========================================================================
    // Errors
    // =========================================================================

    /// @notice `renounceOwnership` is blocked; see the contract notes.
    error RenounceOwnershipBlocked();

    // =========================================================================
    // Storage
    // =========================================================================

    /// @dev Entries in append order. Index i is stable forever once written.
    Entry[] private _entries;

    /// @inheritdoc IRgbRejectList
    address public override appender;

    // =========================================================================
    // Constructor
    // =========================================================================

    /// @param owner_    Initial owner, who appoints and rotates the appender.
    ///                  Must be non-zero (enforced by `Ownable`).
    /// @param appender_ Initial appender, allowed to append entries. Must be
    ///                  non-zero.
    constructor(address owner_, address appender_) Ownable(owner_) {
        _setAppender(appender_);
    }

    // =========================================================================
    // Modifiers
    // =========================================================================

    modifier onlyAppender() {
        if (msg.sender != appender) revert NotAppender(msg.sender);
        _;
    }

    // =========================================================================
    // Appender-only
    // =========================================================================

    /// @inheritdoc IRgbRejectList
    function append(Entry[] calldata batch) external override onlyAppender {
        if (batch.length == 0) revert EmptyBatch();

        uint256 index = _entries.length;
        for (uint256 i = 0; i < batch.length; i++) {
            Entry calldata entry = batch[i];
            if (entry.opId == bytes32(0)) revert InvalidOpId();

            _entries.push(entry);
            emit EntryAdded(index + i, entry.opId, entry.assignmentType, entry.no, entry.reject);
        }
    }

    // =========================================================================
    // Owner-only
    // =========================================================================

    /// @inheritdoc IRgbRejectList
    function setAppender(address newAppender) external override onlyOwner {
        _setAppender(newAppender);
    }

    /// @notice Permanently blocked; see the contract notes.
    function renounceOwnership() public view override onlyOwner {
        revert RenounceOwnershipBlocked();
    }

    // =========================================================================
    // Views
    // =========================================================================

    /// @inheritdoc IRgbRejectList
    function length() external view override returns (uint256) {
        return _entries.length;
    }

    /// @inheritdoc IRgbRejectList
    function entries(uint256 start, uint256 end) external view override returns (Entry[] memory page) {
        uint256 len = _entries.length;
        if (end > len) end = len;
        if (start >= end) return new Entry[](0);

        page = new Entry[](end - start);
        for (uint256 i = start; i < end; i++) {
            page[i - start] = _entries[i];
        }
    }

    // =========================================================================
    // Internal
    // =========================================================================

    function _setAppender(address newAppender) private {
        if (newAppender == address(0)) revert InvalidAppender();
        emit AppenderUpdated(appender, newAppender);
        appender = newAppender;
    }
}
