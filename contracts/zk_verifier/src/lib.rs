#![no_std]

//! # `zk_verifier`
//!
//! On-chain ZK proof verifier for KYC compliance proofs.
//!
//! ## AZ-014 fix — bounded storage
//!
//! The original implementation stored all nullifiers and all verified-wallet
//! addresses in two growing `Map` values held in **instance** storage.  Every
//! verification call appended to those maps; entries were never removed.
//! Because instance storage is size-bounded and its rent scales with the total
//! byte size of the instance, the contract would eventually hit storage limits
//! or impose unbounded rent on honest users — a DoS vector.
//!
//! This implementation uses **persistent** storage keyed *per entry*:
//!
//! ```text
//! DataKey::ScopedNullifier(BytesN<32>, BytesN<32>) → bool
//! DataKey::Verified(Address)     → VerificationRecord (fixed validity deadline)
//! DataKey::AttestedCommitment(BytesN<32>) → bool
//! DataKey::Policy                → PolicyParams        (instance — bounded scalar)
//! DataKey::Admin                 → Address             (instance — bounded scalar)
//! DataKey::Paused                → bool                (instance — bounded scalar)
//! ```
//!
//! ## W10-SEC-002 — on-chain proof and policy enforcement
//!
//! Previously `required_kyc` and `allowed_country` were `pub` inputs supplied
//! entirely by the prover.  Nothing stopped a prover from setting
//! `required_kyc = 0` and `allowed_country = <their own value>` to trivially
//! satisfy both circuit constraints and get a "valid" proof with zero real KYC.
//!
//! The contract delegates proof verification to a configured
//! `zk_proof_verifier` contract and independently checks the verified public
//! inputs against an admin-controlled `PolicyParams` value:
//!
//! * `public_inputs[1]` (required_kyc) **≥** `policy.min_tier`
//! * `public_inputs[2]` (country_code) **==** `policy.allowed_country`
//!
//! Until both policy and proof-verifier contracts are configured, verification
//! fails closed.

use shared::ContractError;
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{
    contract, contractimpl, contracttype, panic_with_error, symbol_short, Address, Bytes, BytesN,
    Env, Vec,
};

// ---------------------------------------------------------------------------
// Storage TTL constants
// ---------------------------------------------------------------------------

/// Ledgers a nullifier entry is kept alive after its last verification.
///
/// Stellar closes ~1 ledger every 5 seconds.
/// 1 051 200 ledgers ≈ 60 days — long enough to prevent replay within any
/// realistic settlement window while still letting stale entries expire.
const NULLIFIER_TTL_LEDGERS: u32 = 1_051_200; // ~60 days

/// TTL bump threshold: extend only when fewer than this many ledgers remain.
/// Set to half the full TTL so we don't bump on every single call.
const NULLIFIER_TTL_THRESHOLD: u32 = NULLIFIER_TTL_LEDGERS / 2;

/// TTL for the instance storage (admin, paused flag, policy).
const INSTANCE_TTL_LEDGERS: u32 = 5_256_000; // ~1 year

/// How long a successful verification record is valid.
///
/// 1 051 200 ledgers ≈ 60 days.  The admin may revoke earlier.
const VERIFICATION_VALIDITY_LEDGERS: u32 = 1_051_200; // ~60 days

/// Public KYC policy bounds shared with the ZK-Comply circuit.
const MIN_KYC_TIER: u128 = 1;
const MAX_KYC_TIER: u128 = 3;
const MIN_COUNTRY_CODE: u128 = 1;
const MAX_COUNTRY_CODE: u128 = 999;

/// Public input layout authenticated by the ZK proof.
///
/// | Index | Field                 | Description                              |
/// |-------|-----------------------|------------------------------------------|
/// |   0   | `commitment`          | Poseidon2 credential commitment            |
/// |   1   | `required_kyc`        | Proved minimum KYC tier (1–3)              |
/// |   2   | `allowed_country`     | Proved ISO-3166-1 numeric country code     |
/// |   3   | `nullifier`           | Poseidon2 commitment/salt/wallet nullifier |
/// |   4   | `wallet_address_hash` | sha256(wallet_xdr)[0..16] as u128          |
const PUBLIC_INPUT_BYTES: u32 = 5 * 32;
const ZK_PROOF_BYTES: u32 = 507 * 32;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Compliance policy stored on-chain by the contract administrator.
///
/// AZ-001: these values are the authoritative compliance requirements.
/// The contract enforces them independently of whatever the prover encodes
/// in the circuit's public inputs, so a malicious prover cannot self-select
/// a weaker policy.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyParams {
    /// Minimum KYC tier required.  Wallets whose proof encodes a tier below
    /// this value are rejected at the contract level regardless of circuit
    /// satisfiability.
    pub min_tier: u128,
    /// ISO-3166-1 numeric country code that is permitted to transact.
    /// The prover's `allowed_country` in `public_inputs[2]` must equal this
    /// exactly; any other country is denied at the contract level.
    pub allowed_country: u128,
}

/// On-chain record created for each successfully verified wallet.
#[contracttype]
#[derive(Clone, Debug)]
pub struct VerificationRecord {
    /// The ledger sequence number at which this record expires.
    pub expires_at_ledger: u32,
    /// The nullifier that was spent for this verification.
    pub nullifier: BytesN<32>,
    /// Snapshot of the policy that was active when the proof was verified.
    /// Stored for auditability.
    pub policy: PolicyParams,
}

/// Event emitted on successful verification.
#[contracttype]
pub struct VerifiedEvent {
    pub user: Address,
    pub nullifier: BytesN<32>,
    pub policy: PolicyParams,
    pub expires_at_ledger: u32,
}

/// Event emitted when the admin revokes a wallet's verification.
#[contracttype]
pub struct VerificationRevokedEvent {
    pub user: Address,
    pub revoked_at_ledger: u32,
}

/// Event emitted when the admin updates the compliance policy.
#[contracttype]
pub struct PolicyUpdatedEvent {
    pub min_tier: u128,
    pub allowed_country: u128,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Contract administrator address.
    Admin,
    /// Whether the contract is paused.
    Paused,
    /// AZ-001: on-chain compliance policy (min_tier + allowed_country).
    Policy,
    /// Address of the separate ZK-enabled UltraHonk verifier contract.
    ProofVerifier,
    /// Spent nullifier — keyed per (commitment, nullifier) pair.
    ///
    /// Stored in *persistent* storage so entries expire individually via TTL
    /// rather than accumulating in a single unbounded instance `Map`.
    ScopedNullifier(BytesN<32>, BytesN<32>),
    /// Verified wallet record — keyed per address and bounded by its deadline.
    ///
    /// Stored in *persistent* storage for the same reason as `ScopedNullifier`.
    Verified(Address),
    /// Attested credential commitment — keyed per 32-byte commitment.
    ///
    /// AZ-002: commitments recorded by the trusted KYC authority (the
    /// admin). Stored in *persistent* storage so the registry stays bounded.
    AttestedCommitment(BytesN<32>),
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct ZkVerifier;

#[contractimpl]
impl ZkVerifier {
    // ── Initialisation ──────────────────────────────────────────────────────

    /// Initialise the contract.  Must be called exactly once.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, ContractError::Unauthorized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        // Policy and proof-verifier address start unconfigured. Verification
        // remains disabled until the admin configures both.
        Self::extend_instance_ttl(&env);
    }

    // ── Compliance policy (AZ-001) ──────────────────────────────────────────

    /// Configure or update the compliance policy.  Only the admin may call this.
    ///
    /// `min_tier` is the minimum KYC tier (1 = basic, 2 = enhanced,
    /// 3 = enterprise). Tier 0 is never accepted.
    ///
    /// `allowed_country` is the ISO-3166-1 numeric code of the only jurisdiction
    /// permitted to verify (e.g. 566 = Nigeria, 404 = Kenya). The code must be
    /// in the circuit's supported numeric range; wildcard policies are not
    /// supported.
    ///
    /// AZ-001: these values are stored on-chain and enforced independently
    /// of the policy assertions included in the proof.
    pub fn set_policy(env: Env, min_tier: u128, allowed_country: u128) {
        Self::check_admin(&env);
        if !(MIN_KYC_TIER..=MAX_KYC_TIER).contains(&min_tier)
            || !(MIN_COUNTRY_CODE..=MAX_COUNTRY_CODE).contains(&allowed_country)
        {
            panic_with_error!(&env, ContractError::InvalidCompliancePolicy);
        }

        let policy = PolicyParams {
            min_tier,
            allowed_country,
        };
        env.storage().instance().set(&DataKey::Policy, &policy);
        env.events().publish(
            (symbol_short!("pol_set"),),
            PolicyUpdatedEvent {
                min_tier,
                allowed_country,
            },
        );
        Self::extend_instance_ttl(&env);
    }

    /// Return the current compliance policy, or `None` if not yet configured.
    pub fn get_policy(env: Env) -> Option<PolicyParams> {
        env.storage().instance().get(&DataKey::Policy)
    }

    /// Configure the contract that performs cryptographic ZK proof checks.
    pub fn set_proof_verifier(env: Env, proof_verifier: Address) {
        Self::check_admin(&env);
        if proof_verifier == env.current_contract_address() {
            panic_with_error!(&env, ContractError::InvalidProofVerifier);
        }
        env.storage()
            .instance()
            .set(&DataKey::ProofVerifier, &proof_verifier);
        Self::extend_instance_ttl(&env);
    }

    pub fn get_proof_verifier(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::ProofVerifier)
    }

    // ── Proof verification ──────────────────────────────────────────────────

    /// AZ-002 — trusted commitment registry.
    ///
    /// The admin acts as the trusted KYC authority: after a user's redacted
    /// KYC review is approved, it records the user's credential commitment
    /// `poseidon2(kyc_level, country_code, salt)` — derived from the
    /// authority's **own** verified records, never from user-claimed values.
    ///
    /// Only attested commitments are accepted by `verify`, so an unverified
    /// user can no longer claim an arbitrary `kyc_level` and produce a valid
    /// proof about a self-asserted credential.
    pub fn register_commitment(env: Env, commitment: BytesN<32>) {
        Self::check_admin(&env);
        Self::assert_not_paused(&env);

        if env
            .storage()
            .persistent()
            .has(&DataKey::AttestedCommitment(commitment.clone()))
        {
            panic_with_error!(&env, ContractError::CommitmentAlreadyAttested);
        }

        env.storage()
            .persistent()
            .set(&DataKey::AttestedCommitment(commitment.clone()), &true);
        env.storage().persistent().extend_ttl(
            &DataKey::AttestedCommitment(commitment.clone()),
            NULLIFIER_TTL_THRESHOLD,
            NULLIFIER_TTL_LEDGERS,
        );

        env.events()
            .publish((symbol_short!("attested"), commitment.clone()), ());

        Self::extend_instance_ttl(&env);
    }

    /// Returns `true` if `commitment` was attested by the trusted KYC authority.
    pub fn is_attested(env: Env, commitment: BytesN<32>) -> bool {
        let key = DataKey::AttestedCommitment(commitment);
        if env.storage().persistent().has(&key) {
            env.storage().persistent().extend_ttl(
                &key,
                NULLIFIER_TTL_THRESHOLD,
                NULLIFIER_TTL_LEDGERS,
            );
            true
        } else {
            false
        }
    }

    /// Proof-less legacy entrypoint. It always fails closed.
    pub fn verify(
        env: Env,
        _wallet: Address,
        _nullifier: BytesN<32>,
        _commitment: BytesN<32>,
        _public_inputs: Vec<u128>,
    ) {
        panic_with_error!(&env, ContractError::ProofRequired);
    }

    /// Record a successful cryptographic proof verification for `wallet`.
    ///
    /// # AZ-001 — server-side compliance policy enforcement
    ///
    /// The configured proof verifier cryptographically verifies `proof` over
    /// `public_inputs` before this contract records the wallet. This contract
    /// then reads its stored `PolicyParams` and checks:
    ///
    /// * `public_inputs[1]` (the tier the prover proves) **≥** `policy.min_tier`
    /// * `public_inputs[2]` (the country the prover proves) **==** `policy.allowed_country`
    ///
    /// Public inputs are assertions authenticated by the ZK proof, not policy
    /// values. They must also satisfy the admin-controlled on-chain policy.
    /// If policy or the proof verifier is not configured, verification fails.
    ///
    /// # AZ-002
    ///
    /// The credential `commitment` must have been attested by the KYC authority.
    ///
    /// # AZ-014
    ///
    /// Both the nullifier and the verified-wallet flag are written to
    /// **persistent** storage as individual scalar entries, not appended to a
    /// shared `Map` in instance storage.
    ///
    /// # AZ-032 — Caller binding
    ///
    /// `public_inputs[4]` must equal the first 16 bytes of `sha256(wallet_xdr)`
    /// interpreted as a little-endian `u128`.
    pub fn verify_proof(
        env: Env,
        wallet: Address,
        nullifier: BytesN<32>,
        commitment: BytesN<32>,
        proof: Bytes,
        public_inputs: Bytes,
    ) {
        wallet.require_auth();
        Self::assert_not_paused(&env);

        if public_inputs.len() != PUBLIC_INPUT_BYTES || proof.len() != ZK_PROOF_BYTES {
            panic_with_error!(&env, ContractError::InvalidPublicInputsLength);
        }

        let proof_commitment = Self::public_input_bytesn(&env, &public_inputs, 0);
        let proof_nullifier = Self::public_input_bytesn(&env, &public_inputs, 3);
        if proof_commitment != commitment || proof_nullifier != nullifier {
            panic_with_error!(&env, ContractError::ProofPublicInputMismatch);
        }

        // Verify wallet_address_hash (public_inputs[4]) matches caller.
        let wallet_xdr: Bytes = wallet.clone().to_xdr(&env);
        let digest: BytesN<32> = env.crypto().sha256(&wallet_xdr).into();
        let digest_bytes = digest.to_array();
        let mut hash_u128: u128 = 0u128;
        let mut i: u32 = 0;
        while i < 16 {
            hash_u128 |= (digest_bytes[i as usize] as u128) << (i * 8);
            i += 1;
        }
        let claimed_wallet_hash = Self::public_input_u128(&env, &public_inputs, 4);
        if hash_u128 != claimed_wallet_hash {
            panic_with_error!(&env, ContractError::ProofCallerMismatch);
        }

        // AZ-001: Load the admin-controlled compliance policy.  Reject all
        // verifications until the policy is explicitly configured.
        let policy: PolicyParams = env
            .storage()
            .instance()
            .get(&DataKey::Policy)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::PolicyNotConfigured));

        // Reject legacy or otherwise invalid policy values as well as invalid
        // prover inputs; a stored policy must never make tier 0 acceptable.
        if !(MIN_KYC_TIER..=MAX_KYC_TIER).contains(&policy.min_tier)
            || !(MIN_COUNTRY_CODE..=MAX_COUNTRY_CODE).contains(&policy.allowed_country)
        {
            panic_with_error!(&env, ContractError::InvalidCompliancePolicy);
        }

        // AZ-001: Enforce minimum KYC tier server-side.
        // public_inputs[1] is the minimum tier asserted by the proof. It must
        // satisfy the system minimum and the admin-configured policy.
        let prover_tier = Self::public_input_u128(&env, &public_inputs, 1);
        if prover_tier < policy.min_tier || prover_tier > MAX_KYC_TIER {
            panic_with_error!(&env, ContractError::KycTierTooLow);
        }

        // AZ-001: Enforce allowed-country server-side.
        // public_inputs[2] is the jurisdiction asserted by the proof; it must match
        // exactly the country stored in the contract policy.
        let prover_country = Self::public_input_u128(&env, &public_inputs, 2);
        if prover_country != policy.allowed_country {
            panic_with_error!(&env, ContractError::CountryNotAllowed);
        }

        // AZ-002: Reject proofs whose commitment was never attested.
        if !env
            .storage()
            .persistent()
            .has(&DataKey::AttestedCommitment(commitment.clone()))
        {
            panic_with_error!(&env, ContractError::CommitmentNotAttested);
        }

        // Reject replayed nullifiers for this commitment.
        if env
            .storage()
            .persistent()
            .has(&DataKey::ScopedNullifier(commitment.clone(), nullifier.clone()))
        {
            panic_with_error!(&env, ContractError::NullifierAlreadySpent);
        }

        let proof_verifier: Address = env
            .storage()
            .instance()
            .get(&DataKey::ProofVerifier)
            .unwrap_or_else(|| {
                panic_with_error!(&env, ContractError::ProofVerifierNotConfigured)
            });
        let proof_valid: bool = env.invoke_contract(
            &proof_verifier,
            &symbol_short!("verify"),
            Vec::from_array(
                &env,
                [proof.into_val(&env), public_inputs.clone().into_val(&env)],
            ),
        );
        if !proof_valid {
            panic_with_error!(&env, ContractError::ProofVerificationFailed);
        }

        // Record the nullifier in persistent storage (AZ-014).
        env.storage()
            .persistent()
            .set(&DataKey::ScopedNullifier(commitment.clone(), nullifier.clone()), &true);
        env.storage().persistent().extend_ttl(
            &DataKey::ScopedNullifier(commitment.clone(), nullifier.clone()),
            NULLIFIER_TTL_THRESHOLD,
            NULLIFIER_TTL_LEDGERS,
        );

        // Store an immutable validity deadline.
        let expires_at_ledger = env
            .ledger()
            .sequence()
            .saturating_add(VERIFICATION_VALIDITY_LEDGERS);
        let record = VerificationRecord {
            expires_at_ledger,
            nullifier: nullifier.clone(),
            policy: policy.clone(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Verified(wallet.clone()), &record);
        env.storage().persistent().extend_ttl(
            &DataKey::Verified(wallet.clone()),
            VERIFICATION_VALIDITY_LEDGERS / 2,
            VERIFICATION_VALIDITY_LEDGERS,
        );

        env.events().publish(
            (symbol_short!("verified"),),
            VerifiedEvent {
                user: wallet,
                nullifier,
                policy,
                expires_at_ledger,
            },
        );

        Self::extend_instance_ttl(&env);
    }

    // ── Read helpers ────────────────────────────────────────────────────────

    /// Returns `true` only while `wallet` has a non-expired verification.
    pub fn is_verified(env: Env, wallet: Address) -> bool {
        let key = DataKey::Verified(wallet);
        env.storage()
            .persistent()
            .get::<_, VerificationRecord>(&key)
            .is_some_and(|record| env.ledger().sequence() < record.expires_at_ledger)
    }

    /// Return the current verification record when it is still valid.
    pub fn verification(env: Env, wallet: Address) -> Option<VerificationRecord> {
        let key = DataKey::Verified(wallet);
        env.storage()
            .persistent()
            .get::<_, VerificationRecord>(&key)
            .filter(|record| env.ledger().sequence() < record.expires_at_ledger)
    }

    /// Returns `true` if `nullifier` has already been spent for `commitment`.
    pub fn is_nullifier_spent(env: Env, commitment: BytesN<32>, nullifier: BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::ScopedNullifier(commitment, nullifier))
    }

    /// Return the admin address.
    pub fn admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::Unauthorized))
    }

    /// Return whether the contract is paused.
    pub fn paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    // ── Admin ───────────────────────────────────────────────────────────────

    /// Pause the contract.
    pub fn pause(env: Env) {
        Self::check_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &true);
        Self::extend_instance_ttl(&env);
    }

    /// Unpause the contract.
    pub fn unpause(env: Env) {
        Self::check_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &false);
        Self::extend_instance_ttl(&env);
    }

    /// Revoke a wallet's verification immediately.
    pub fn revoke_verification(env: Env, wallet: Address) -> bool {
        Self::check_admin(&env);
        let key = DataKey::Verified(wallet.clone());
        if !env.storage().persistent().has(&key) {
            return false;
        }

        env.storage().persistent().remove(&key);
        env.events().publish(
            (symbol_short!("revoked"),),
            VerificationRevokedEvent {
                user: wallet,
                revoked_at_ledger: env.ledger().sequence(),
            },
        );
        Self::extend_instance_ttl(&env);
        true
    }

    // ── Internal helpers ────────────────────────────────────────────────────

    fn check_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, ContractError::Unauthorized));
        admin.require_auth();
    }

    fn public_input_bytesn(env: &Env, public_inputs: &Bytes, index: u32) -> BytesN<32> {
        let mut field = [0u8; 32];
        let start = index * 32;
        let mut offset = 0;
        while offset < 32 {
            field[offset as usize] = public_inputs.get_unchecked(start + offset);
            offset += 1;
        }
        BytesN::from_array(env, &field)
    }

    fn public_input_u128(env: &Env, public_inputs: &Bytes, index: u32) -> u128 {
        let start = index * 32;
        let mut offset = 0;
        while offset < 16 {
            if public_inputs.get_unchecked(start + offset) != 0 {
                panic_with_error!(env, ContractError::InvalidPublicInputValue);
            }
            offset += 1;
        }

        let mut value = 0u128;
        while offset < 32 {
            value = (value << 8) | public_inputs.get_unchecked(start + offset) as u128;
            offset += 1;
        }
        value
    }

    fn assert_not_paused(env: &Env) {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if paused {
            panic_with_error!(env, ContractError::Paused);
        }
    }

    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_LEDGERS / 2, INSTANCE_TTL_LEDGERS);
    }
}
