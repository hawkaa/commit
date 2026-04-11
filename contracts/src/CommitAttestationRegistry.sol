// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

/// @title CommitAttestationRegistry
/// @notice Append-only registry of endorsement attestation hashes for the Commit trust network.
///         Stores proof hashes on-chain for public verifiability. Only the owner (backend service)
///         can submit attestations. Endorsement data stays in the off-chain database; only the
///         cryptographic commitment lands here.
contract CommitAttestationRegistry {
    address public owner;

    struct Attestation {
        bytes32 proofHash;
        uint64 timestamp;
    }

    mapping(bytes32 => Attestation) public attestations;

    event AttestationRecorded(
        bytes32 indexed endorsementId,
        bytes32 proofHash,
        uint64 timestamp
    );

    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);

    modifier onlyOwner() {
        _checkOwner();
        _;
    }

    function _checkOwner() internal view {
        require(msg.sender == owner, "unauthorized");
    }

    constructor() {
        owner = msg.sender;
    }

    /// @notice Record a single attestation.
    /// @param endorsementId Deterministic bytes32 derived from the off-chain endorsement UUID.
    /// @param proofHash     SHA-256 hash of the endorsement proof.
    function attest(bytes32 endorsementId, bytes32 proofHash) external onlyOwner {
        require(attestations[endorsementId].timestamp == 0, "already attested");
        uint64 ts = uint64(block.timestamp);
        attestations[endorsementId] = Attestation({ proofHash: proofHash, timestamp: ts });
        emit AttestationRecorded(endorsementId, proofHash, ts);
    }

    /// @notice Record a batch of attestations in a single transaction.
    function attestBatch(
        bytes32[] calldata endorsementIds,
        bytes32[] calldata proofHashes
    ) external onlyOwner {
        require(endorsementIds.length == proofHashes.length, "length mismatch");
        uint64 ts = uint64(block.timestamp);
        for (uint256 i = 0; i < endorsementIds.length; i++) {
            bytes32 eid = endorsementIds[i];
            require(attestations[eid].timestamp == 0, "already attested");
            attestations[eid] = Attestation({ proofHash: proofHashes[i], timestamp: ts });
            emit AttestationRecorded(eid, proofHashes[i], ts);
        }
    }

    /// @notice Check whether a given proof hash matches the on-chain attestation.
    function verify(bytes32 endorsementId, bytes32 proofHash) external view returns (bool) {
        return attestations[endorsementId].proofHash == proofHash;
    }

    /// @notice Transfer contract ownership to a new address.
    function transferOwnership(address newOwner) external onlyOwner {
        require(newOwner != address(0), "zero address");
        emit OwnershipTransferred(owner, newOwner);
        owner = newOwner;
    }
}
