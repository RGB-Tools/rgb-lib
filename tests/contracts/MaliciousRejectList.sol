// SPDX-License-Identifier: MIT
pragma solidity 0.8.35;

import {IRgbRejectList} from "./interfaces/IRgbRejectList.sol";

/// @title MaliciousRejectList
/// @notice Test-only reject list that breaks the `IRgbRejectList` read model in the way selected
///         at deployment, as a sender declaring it in a BFA contract could do to attack the
///         receivers of the asset.
contract MaliciousRejectList {
    enum Mode {
        // `length` reverts
        LengthReverts,
        // `entries` reverts
        EntriesReverts,
        // `entries` returns fewer entries than requested
        ShortPage,
        // `entries` returns a bool word other than 0 or 1
        BadBool,
        // `length` doesn't fit in a uint64
        LengthOverflow,
        // the list shrinks at every block, so it is shorter at the latest block than at the
        // finalized one
        Shrinking,
        // `length` is the largest uint64 and every page is served in full
        HugeLength
    }

    Mode public immutable mode;
    uint256 private immutable deployedAt;

    constructor(Mode mode_) {
        mode = mode_;
        deployedAt = block.number;
    }

    function length() external view returns (uint256) {
        if (mode == Mode.LengthReverts) revert();
        if (mode == Mode.LengthOverflow) return uint256(type(uint64).max) + 1;
        if (mode == Mode.Shrinking) return deployedAt + 1_000_000 - block.number;
        if (mode == Mode.HugeLength) return type(uint64).max;
        return 1;
    }

    function entries(uint256 start, uint256 end) external view returns (IRgbRejectList.Entry[] memory page) {
        if (mode == Mode.EntriesReverts) revert();
        if (mode == Mode.ShortPage) return new IRgbRejectList.Entry[](0);
        if (mode == Mode.BadBool) {
            // one entry for a non-zero opId, with 2 as its `reject` bool
            assembly {
                let p := mload(0x40)
                mstore(p, 0x20)
                mstore(add(p, 0x20), 1)
                mstore(add(p, 0x40), 1)
                mstore(add(p, 0x60), 0)
                mstore(add(p, 0x80), 0)
                mstore(add(p, 0xa0), 2)
                return(p, 0xc0)
            }
        }
        page = new IRgbRejectList.Entry[](end - start);
        for (uint256 i = 0; i < page.length; i++) {
            page[i] = IRgbRejectList.Entry(bytes32(start + i + 1), 0, 0, false);
        }
    }
}
