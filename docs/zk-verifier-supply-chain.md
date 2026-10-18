# ZK Verifier Supply Chain Security (W10-Z-009)

**Issue:** W10-Z-009  
**Severity:** Warning  
**Area:** zk/supply-chain  
**Status:** Documented

---

## Overview

The ZK proof verification in this protocol relies on UltraHonk proof system compatibility. The current implementation uses a stub verifier that tracks nullifiers and commitments but does not perform cryptographic proof verification on-chain.

## Supply Chain Requirements

### Verifier Library Pinning

If integrating an external Rust verifier library (e.g., `ultrahonk_rust_verifier`), the following security requirements must be met:

1. **Exact revision pin**: Dependencies must specify an exact git commit hash or published crate version
2. **Provenance documentation**: Record the source repository, fork maintainer, and audit status
3. **Vendoring or mirroring**: Critical dependencies should be vendored or mirrored to prevent supply chain attacks
4. **No unpinned git dependencies**: All git-based dependencies must include `rev = "<commit-hash>"` or `tag = "<version>"`

### Example Secure Configuration

If the verifier were to be integrated, the configuration should look like:

```toml
[dependencies]
# Option 1: Published crate (preferred)
ultrahonk_rust_verifier = "=0.1.0"

# Option 2: Pinned git dependency (if no crate available)
# ultrahonk_rust_verifier = { git = "https://github.com/maintainer/ultrahonk_rust_verifier", rev = "2a73c4ba8f1e3d5c9b7a2e4f6d8c1a3b5e7f9d2c" }
```

### Audit Trail

Any external verifier library must include:

- **Source repository**: Full URL to the canonical or forked repository
- **Fork rationale**: If using a fork, document why the fork exists and who maintains it
- **Audit status**: Record whether the library has undergone security review
- **Trusted setup**: Document the trusted setup ceremony provenance for the proof system
- **Version compatibility**: Ensure compatibility with the pinned Noir/Barretenberg toolchain (v1.0.0-beta.9 / v0.87.0)

## Current State

The current `zk_verifier` contract (contracts/zk_verifier/src/lib.rs) implements:

- ✅ Nullifier tracking (prevents double-spend)
- ✅ Commitment attestation (trusted KYC authority model)
- ✅ Caller binding via wallet hash (prevents proof reuse)
- ⚠️ **No on-chain cryptographic proof verification**

The contract relies on off-chain proof generation and assumes proofs submitted are valid. This is acceptable for the current trust model where:

1. The KYC authority (admin) attests commitments
2. Users prove knowledge of an attested credential
3. The nullifier prevents credential reuse

## Integration Path

If on-chain cryptographic verification is required:

1. Select a verifier library (e.g., ultrahonk_rust_verifier or equivalent)
2. Verify the library is compatible with Noir 1.0.0-beta.9 / Barretenberg v0.87.0
3. Pin to an exact revision or published crate version
4. Document provenance and audit status in this file
5. Add the dependency to contracts/zk_verifier/Cargo.toml
6. Implement the verification call in the `verify` method
7. Update gas cost estimates and contract size checks
8. Add integration tests with real proof artifacts

## References

- Noir/Barretenberg toolchain pin: contracts/zk_verifier/README.md
- Proof generation: sdk/generate-artifacts.mjs
- Circuit source: zk/circuits/ (if present)
- Workspace supply chain policy: Cargo.toml (workspace-level comments)
