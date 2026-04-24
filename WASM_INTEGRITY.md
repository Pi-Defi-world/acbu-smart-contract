# WASM Integrity Verification

## Overview

This document describes the WASM artifact integrity verification process for ACBU smart contracts. This is a critical security control to prevent supply chain attacks through WASM substitution.

## Problem Statement

**Severity**: Medium  
**Area**: contracts/build  
**Affected Contracts**: acbu_minting, acbu_burning, acbu_reserve_tracker  
**Risk**: Integrity not verified; supply chain risk

The token WASM contract (`soroban_token_contract.wasm`) is imported by three critical contracts using the Soroban SDK's `contractimport!` macro. Without proper hash verification, an attacker could:

1. Replace the WASM artifact with a malicious version
2. Modify the artifact during build or deployment
3. Inject unauthorized token operations (mint, burn, transfer)
4. Compromise the entire ACBU ecosystem

## Solution Architecture

### 1. Hash Pinning

**Token WASM Hash**: `6b14997b915dee21082884cd5a2f1f2f0aef0073d1dcb9c5b3c674cf487fb41d`

This hash is pinned in three locations:

- `acbu_minting/src/lib.rs` - contractimport! macro
- `acbu_burning/src/lib.rs` - contractimport! macro
- `acbu_reserve_tracker/src/lib.rs` - contractimport! macro

### 2. Build-Time Verification

**File**: `build.rs`

The Rust build script runs before compilation and:
- Verifies the WASM file exists
- Checks hash consistency across all contract imports
- Fails the build if any hash mismatches are detected

**Behavior**: Build fails immediately if integrity check fails

### 3. CI/CD Pipeline Verification

**File**: `.github/workflows/verify-wasm-integrity.yml`

GitHub Actions workflow runs on:
- Push to main/develop branches
- Pull requests affecting WASM or contract imports
- Changes to Cargo.lock

**Verification Steps**:
1. Verify WASM artifact hash matches expected value
2. Verify hash consistency across all three contracts
3. Check for WASM modifications in git history
4. Build all contracts with hash verification
5. Verify all build artifacts are generated
6. Upload artifacts for deployment verification

### 4. Deployment Verification

**File**: `scripts/verify_wasm_hash.sh`

Pre-deployment script that:
- Verifies WASM artifact integrity before deployment
- Fails fast if hash mismatches
- Provides clear error messages for remediation

**Usage**:
```bash
./scripts/verify_wasm_hash.sh
```

## Verification Process

### Local Development

Before building locally:

```bash
# Verify WASM integrity
bash scripts/verify_wasm_hash.sh

# Build contracts (build.rs will also verify)
cargo build --target wasm32-unknown-unknown --release
```

### Continuous Integration

The GitHub Actions workflow automatically:
1. Verifies WASM hash on every push/PR
2. Builds all contracts with verification
3. Uploads artifacts for audit trail
4. Fails the build if any check fails

### Deployment

Before deploying to testnet/mainnet:

```bash
# Verify WASM integrity
bash scripts/verify_wasm_hash.sh

# Deploy contracts
./scripts/deploy_testnet.sh
```

## Updating the WASM Hash

If the token contract is intentionally updated:

### 1. Obtain New Hash

```bash
sha256sum soroban_token_contract.wasm
```

### 2. Update All Three Locations

**acbu_minting/src/lib.rs**:
```rust
#[allow(dead_code)]
pub mod token_contract {
    soroban_sdk::contractimport!(
        file = "../soroban_token_contract.wasm",
        sha256 = "NEW_HASH_HERE"
    );
}
```

**acbu_burning/src/lib.rs**:
```rust
#[allow(dead_code)]
pub mod token_contract {
    soroban_sdk::contractimport!(
        file = "../soroban_token_contract.wasm",
        sha256 = "NEW_HASH_HERE"
    );
}
```

**acbu_reserve_tracker/src/lib.rs**:
```rust
#[allow(dead_code)]
pub mod token_contract {
    soroban_sdk::contractimport!(
        file = "../soroban_token_contract.wasm",
        sha256 = "NEW_HASH_HERE"
    );
}
```

### 3. Update Verification Scripts

**scripts/verify_wasm_hash.sh**:
```bash
EXPECTED_HASH="NEW_HASH_HERE"
```

**.github/workflows/verify-wasm-integrity.yml**:
```yaml
EXPECTED_HASH="NEW_HASH_HERE"
```

### 4. Verify Consistency

```bash
# Build will verify all hashes match
cargo build --target wasm32-unknown-unknown --release
```

### 5. Create PR with Changes

- Include the new WASM artifact
- Include all hash updates
- Document the reason for the update
- Require security review before merge

## Acceptance Criteria

✅ **Build fails if hash mismatches artifact**
- `build.rs` verifies hash consistency before compilation
- CI/CD pipeline verifies WASM hash on every push/PR
- Deployment script verifies hash before deployment

✅ **Hash is pinned in all three contracts**
- acbu_minting: `sha256 = "6b14997b915dee21082884cd5a2f1f2f0aef0073d1dcb9c5b3c674cf487fb41d"`
- acbu_burning: `sha256 = "6b14997b915dee21082884cd5a2f1f2f0aef0073d1dcb9c5b3c674cf487fb41d"`
- acbu_reserve_tracker: `sha256 = "6b14997b915dee21082884cd5a2f1f2f0aef0073d1dcb9c5b3c674cf487fb41d"`

✅ **Verification happens at multiple stages**
- Build time: `build.rs`
- CI/CD: GitHub Actions workflow
- Pre-deployment: `verify_wasm_hash.sh`

✅ **Clear error messages for failures**
- Explains the supply chain risk
- Provides remediation steps
- Prevents accidental deployment of compromised artifacts

## Security Considerations

### What This Protects Against

1. **WASM Substitution Attacks**: Prevents deployment of modified token contracts
2. **Build-Time Tampering**: Detects changes to WASM artifact during build
3. **Repository Compromise**: Catches unauthorized WASM modifications in git
4. **Deployment Errors**: Prevents accidental deployment of wrong artifact version

### What This Does NOT Protect Against

1. **Source Code Compromise**: If the WASM source is compromised, the hash will be updated accordingly
2. **Private Key Compromise**: If deployment credentials are compromised, attacker can still deploy
3. **Network-Level Attacks**: Does not protect against MITM attacks during deployment
4. **Soroban SDK Vulnerabilities**: Does not protect against vulnerabilities in the SDK itself

### Defense in Depth

This verification is one layer of defense. Additional security measures:

1. **Code Review**: All WASM updates require security review
2. **Audit Trail**: Git history tracks all WASM changes
3. **Access Control**: Restrict who can merge WASM changes
4. **Deployment Authorization**: Require approval for mainnet deployments
5. **Monitoring**: Monitor contract behavior post-deployment

## Troubleshooting

### Build Fails: "WASM file not found"

**Cause**: `soroban_token_contract.wasm` is missing from project root

**Solution**:
```bash
# Verify file exists
ls -la soroban_token_contract.wasm

# If missing, restore from git
git checkout soroban_token_contract.wasm
```

### Build Fails: "WASM hash mismatch"

**Cause**: WASM artifact has been modified or replaced

**Solution**:
1. Verify the source of the WASM file
2. If intentionally updated, update all three contract imports with new hash
3. Run verification script to confirm

### CI/CD Fails: "Hash mismatch in contract"

**Cause**: Contract imports have inconsistent hashes

**Solution**:
1. Ensure all three contracts use the same hash
2. Run `cargo build` locally to verify
3. Check git diff for hash inconsistencies

## References

- [Soroban SDK Documentation](https://docs.rs/soroban-sdk/)
- [WASM Security Best Practices](https://webassembly.org/docs/security/)
- [Supply Chain Security](https://cheatsheetseries.owasp.org/cheatsheets/Supply_Chain_Security_Cheat_Sheet.html)
