# Security Fix: WASM Hash Verification

## Issue Summary

**Severity**: Medium  
**Area**: contracts/build  
**Affected Contracts**: acbu_minting, acbu_burning, acbu_reserve_tracker  
**Risk Category**: Supply Chain Security  

### Problem

The token WASM contract (`soroban_token_contract.wasm`) is imported by three critical contracts without proper integrity verification. This creates a supply chain vulnerability where:

- An attacker could replace the WASM artifact with a malicious version
- The build process would not detect the substitution
- Compromised contracts could be deployed to production
- Token operations (mint, burn, transfer) could be hijacked

### Root Cause

The `contractimport!` macro in Soroban SDK includes a SHA256 hash parameter, but there was no verification that:
1. The WASM file actually matches the pinned hash
2. The hash is consistent across all three contracts
3. The build fails if hash verification fails

## Solution Implemented

### 1. Build-Time Verification (`build.rs`)

**File**: `acbu-smart-contract/build.rs`

Rust build script that runs before compilation:
- Verifies WASM file exists
- Checks hash consistency across all contract imports
- Fails the build immediately if any hash mismatches are detected

**Behavior**: Build fails with clear error message if integrity check fails

```bash
cargo build --target wasm32-unknown-unknown --release
# Fails if WASM hash doesn't match
```

### 2. CI/CD Pipeline Verification

**File**: `.github/workflows/verify-wasm-integrity.yml`

GitHub Actions workflow that runs on:
- Push to main/develop branches
- Pull requests affecting WASM or contract imports
- Changes to Cargo.lock

**Verification Steps**:
1. Verify WASM artifact hash matches expected value
2. Verify hash consistency across all three contracts
3. Check for WASM modifications in git history
4. Build all contracts with hash verification
5. Verify all build artifacts are generated
6. Upload artifacts for audit trail

**Behavior**: CI/CD pipeline fails if any verification step fails

### 3. Pre-Deployment Verification

**File**: `scripts/verify_wasm_hash.sh`

Standalone script for pre-deployment verification:
- Verifies WASM artifact integrity before deployment
- Fails fast if hash mismatches
- Provides clear error messages for remediation

**Usage**:
```bash
bash scripts/verify_wasm_hash.sh
```

### 4. Deployment Verification

**File**: `scripts/verify_deployment.sh`

Comprehensive verification script that checks:
- All build artifacts exist and are valid
- Token WASM integrity
- Contract imports have correct hashes

**Usage**:
```bash
bash scripts/verify_deployment.sh
```

### 5. Git Hooks

**File**: `.githooks/pre-commit`

Pre-commit hook that prevents committing changes that would break verification:
- Detects WASM artifact modifications
- Verifies all contract imports are updated
- Ensures hash consistency across contracts

**Setup**:
```bash
git config core.hooksPath .githooks
chmod +x .githooks/pre-commit
```

### 6. Documentation

**Files**:
- `WASM_INTEGRITY.md` - Comprehensive guide to WASM verification process
- `SETUP_HOOKS.md` - Instructions for setting up git hooks
- `SECURITY_FIX_SUMMARY.md` - This file

## Pinned Hash

**Token WASM Hash**: `6b14997b915dee21082884cd5a2f1f2f0aef0073d1dcb9c5b3c674cf487fb41d`

This hash is pinned in:
- `acbu_minting/src/lib.rs` - contractimport! macro
- `acbu_burning/src/lib.rs` - contractimport! macro
- `acbu_reserve_tracker/src/lib.rs` - contractimport! macro
- `scripts/verify_wasm_hash.sh` - verification script
- `.github/workflows/verify-wasm-integrity.yml` - CI/CD workflow

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
- Pre-commit: Git hook

✅ **Clear error messages for failures**
- Explains the supply chain risk
- Provides remediation steps
- Prevents accidental deployment of compromised artifacts

## Verification Process

### Local Development

```bash
# 1. Setup git hooks (one-time)
git config core.hooksPath .githooks
chmod +x .githooks/pre-commit

# 2. Verify WASM integrity
bash scripts/verify_wasm_hash.sh

# 3. Build contracts (build.rs will also verify)
cargo build --target wasm32-unknown-unknown --release

# 4. Verify deployment readiness
bash scripts/verify_deployment.sh
```

### Continuous Integration

The GitHub Actions workflow automatically:
1. Verifies WASM hash on every push/PR
2. Builds all contracts with verification
3. Uploads artifacts for audit trail
4. Fails the build if any check fails

### Deployment

```bash
# 1. Verify WASM integrity
bash scripts/verify_wasm_hash.sh

# 2. Verify deployment readiness
bash scripts/verify_deployment.sh

# 3. Deploy contracts
./scripts/deploy_testnet.sh
```

## Updating the WASM Hash

If the token contract is intentionally updated:

### 1. Obtain New Hash

```bash
sha256sum soroban_token_contract.wasm
```

### 2. Update All Locations

Update the hash in:
- `acbu_minting/src/lib.rs`
- `acbu_burning/src/lib.rs`
- `acbu_reserve_tracker/src/lib.rs`
- `scripts/verify_wasm_hash.sh`
- `.github/workflows/verify-wasm-integrity.yml`

### 3. Verify Consistency

```bash
cargo build --target wasm32-unknown-unknown --release
```

### 4. Create PR with Changes

- Include the new WASM artifact
- Include all hash updates
- Document the reason for the update
- Require security review before merge

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

## Files Modified/Created

### New Files

1. `build.rs` - Build script for WASM verification
2. `.github/workflows/verify-wasm-integrity.yml` - CI/CD workflow
3. `scripts/verify_wasm_hash.sh` - Pre-deployment verification
4. `scripts/verify_deployment.sh` - Comprehensive deployment verification
5. `.githooks/pre-commit` - Git pre-commit hook
6. `WASM_INTEGRITY.md` - Comprehensive documentation
7. `SETUP_HOOKS.md` - Git hooks setup guide
8. `SECURITY_FIX_SUMMARY.md` - This file

### Modified Files

1. `Cargo.toml` - Added build script configuration

## Testing the Fix

### Test 1: Verify Build Fails on Hash Mismatch

```bash
# Modify the WASM file
echo "corrupted" >> soroban_token_contract.wasm

# Try to build (should fail)
cargo build --target wasm32-unknown-unknown --release

# Restore the file
git checkout soroban_token_contract.wasm
```

### Test 2: Verify CI/CD Catches Hash Mismatch

```bash
# Modify the WASM file
echo "corrupted" >> soroban_token_contract.wasm

# Try to commit (should fail if hooks are setup)
git add soroban_token_contract.wasm
git commit -m "test"

# Restore the file
git checkout soroban_token_contract.wasm
```

### Test 3: Verify Pre-Deployment Script

```bash
# Run verification script
bash scripts/verify_wasm_hash.sh

# Should output: ✅ PASS: WASM hash verified
```

## Rollout Plan

1. **Immediate**: Deploy build.rs and CI/CD workflow
2. **Week 1**: Setup git hooks in development environment
3. **Week 2**: Document process and train team
4. **Week 3**: Enforce on all branches
5. **Ongoing**: Monitor and maintain verification process

## References

- [Soroban SDK Documentation](https://docs.rs/soroban-sdk/)
- [WASM Security Best Practices](https://webassembly.org/docs/security/)
- [Supply Chain Security](https://cheatsheetseries.owasp.org/cheatsheets/Supply_Chain_Security_Cheat_Sheet.html)
- [OWASP Secure Coding Practices](https://owasp.org/www-project-secure-coding-practices-quick-reference-guide/)

## Questions or Issues?

Refer to `WASM_INTEGRITY.md` for detailed troubleshooting and FAQs.
