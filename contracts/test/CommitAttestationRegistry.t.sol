// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

import {Test} from "forge-std/Test.sol";
import {CommitAttestationRegistry} from "../src/CommitAttestationRegistry.sol";

contract CommitAttestationRegistryTest is Test {
    CommitAttestationRegistry registry;
    address deployer = address(this);
    address stranger = address(0xBEEF);

    function setUp() public {
        registry = new CommitAttestationRegistry();
    }

    // ── attest ──────────────────────────────────────────────────

    function test_attest_stores_correctly() public {
        bytes32 eid = keccak256("endorsement-1");
        bytes32 hash = keccak256("proof-data-1");

        registry.attest(eid, hash);

        (bytes32 storedHash, uint64 storedTs) = registry.attestations(eid);
        assertEq(storedHash, hash);
        assertEq(storedTs, uint64(block.timestamp));
    }

    function test_attest_emits_event() public {
        bytes32 eid = keccak256("endorsement-2");
        bytes32 hash = keccak256("proof-data-2");

        vm.expectEmit(true, false, false, true);
        emit CommitAttestationRegistry.AttestationRecorded(eid, hash, uint64(block.timestamp));

        registry.attest(eid, hash);
    }

    function test_attest_reverts_if_not_owner() public {
        bytes32 eid = keccak256("endorsement-3");
        bytes32 hash = keccak256("proof-data-3");

        vm.prank(stranger);
        vm.expectRevert("unauthorized");
        registry.attest(eid, hash);
    }

    function test_attest_reverts_if_duplicate() public {
        bytes32 eid = keccak256("endorsement-4");
        bytes32 hash = keccak256("proof-data-4");

        registry.attest(eid, hash);

        vm.expectRevert("already attested");
        registry.attest(eid, keccak256("different-proof"));
    }

    // ── attestBatch ─────────────────────────────────────────────

    function test_attestBatch_stores_multiple() public {
        bytes32[] memory eids = new bytes32[](3);
        bytes32[] memory hashes = new bytes32[](3);
        for (uint256 i = 0; i < 3; i++) {
            eids[i] = keccak256(abi.encodePacked("batch-eid-", i));
            hashes[i] = keccak256(abi.encodePacked("batch-hash-", i));
        }

        registry.attestBatch(eids, hashes);

        for (uint256 i = 0; i < 3; i++) {
            (bytes32 storedHash, uint64 storedTs) = registry.attestations(eids[i]);
            assertEq(storedHash, hashes[i]);
            assertEq(storedTs, uint64(block.timestamp));
        }
    }

    function test_attestBatch_reverts_on_length_mismatch() public {
        bytes32[] memory eids = new bytes32[](2);
        bytes32[] memory hashes = new bytes32[](3);

        vm.expectRevert("length mismatch");
        registry.attestBatch(eids, hashes);
    }

    function test_attestBatch_reverts_on_duplicate() public {
        bytes32 eid = keccak256("dup-eid");
        registry.attest(eid, keccak256("original"));

        bytes32[] memory eids = new bytes32[](2);
        bytes32[] memory hashes = new bytes32[](2);
        eids[0] = keccak256("new-eid");
        hashes[0] = keccak256("new-hash");
        eids[1] = eid; // already attested
        hashes[1] = keccak256("dup-hash");

        vm.expectRevert("already attested");
        registry.attestBatch(eids, hashes);
    }

    function test_attestBatch_reverts_if_not_owner() public {
        bytes32[] memory eids = new bytes32[](1);
        bytes32[] memory hashes = new bytes32[](1);
        eids[0] = keccak256("stranger-eid");
        hashes[0] = keccak256("stranger-hash");

        vm.prank(stranger);
        vm.expectRevert("unauthorized");
        registry.attestBatch(eids, hashes);
    }

    // ── verify ──────────────────────────────────────────────────

    function test_verify_returns_true_for_matching_hash() public {
        bytes32 eid = keccak256("verify-eid");
        bytes32 hash = keccak256("verify-hash");

        registry.attest(eid, hash);
        assertTrue(registry.verify(eid, hash));
    }

    function test_verify_returns_false_for_wrong_hash() public {
        bytes32 eid = keccak256("verify-eid-2");
        bytes32 hash = keccak256("correct-hash");

        registry.attest(eid, hash);
        assertFalse(registry.verify(eid, keccak256("wrong-hash")));
    }

    function test_verify_returns_false_for_nonexistent() public view {
        bytes32 eid = keccak256("nonexistent-eid");
        assertFalse(registry.verify(eid, keccak256("any-hash")));
    }

    // ── transferOwnership ───────────────────────────────────────

    function test_transferOwnership() public {
        address newOwner = address(0xCAFE);

        registry.transferOwnership(newOwner);
        assertEq(registry.owner(), newOwner);

        // New owner can attest
        vm.prank(newOwner);
        registry.attest(keccak256("new-owner-eid"), keccak256("new-owner-hash"));

        // Old owner cannot
        vm.expectRevert("unauthorized");
        registry.attest(keccak256("old-owner-eid"), keccak256("old-owner-hash"));
    }

    function test_transferOwnership_rejects_zero_address() public {
        vm.expectRevert("zero address");
        registry.transferOwnership(address(0));
    }

    function test_transferOwnership_emits_event() public {
        address newOwner = address(0xCAFE);

        vm.expectEmit(true, true, false, false);
        emit CommitAttestationRegistry.OwnershipTransferred(deployer, newOwner);

        registry.transferOwnership(newOwner);
    }
}
