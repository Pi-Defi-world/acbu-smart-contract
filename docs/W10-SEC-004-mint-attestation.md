# W10-SEC-004 — Independent fintech-partner attestation for fiat minting

**Issue:** W10-SEC-004 (Pi-Defi-world/acbu-smart-contract#982)
**Area:** security / `acbu_minting`
**Severity:** High
**Status:** Addressed (mitigated by AC-038); regression-tested in [`acbu_minting/tests/test_mint_from_fiat.rs`](../acbu_minting/tests/test_mint_from_fiat.rs)

---

## 1. Finding

The Wave 10 audit reported that `mint_from_fiat` was authorized by a **single** operator
key (`operator.require_auth()` only). A compromise of that one backend key would have
permitted unlimited minting with no genuine, independently verifiable off-chain fiat
confirmation, contradicting the documented security model (“signed confirmation from
fintech partner”, “no single key has unilateral control”).

## 2. Mitigation

`mint_from_fiat` now requires **two independent authorization factors**:

| Factor | Mechanism | Where |
| --- | --- | --- |
| Stellar operator key | `operator == stored operator` **and** `operator.require_auth()` | `acbu_minting/src/lib.rs` (`mint_from_fiat`) |
| Off-chain ed25519 attestation | `FiatSettlementProof { pub_key, signature }` verified on-chain against the admin-registered `operator_pub_key` | `verify_fiat_settlement_proof` |

The attestation commitment is:

```
sha256( XDR(fintech_tx_id) ‖ XDR(recipient) ‖ XDR(fiat_amount) ‖ XDR(currency) ‖ XDR(ledger_timestamp) )
```

Verification fails closed:

- `proof.pub_key != stored_pub_key` → `InvalidFiatSettlementProof` (5032).
- signature does not verify against the stored key → the ed25519 verification traps.
- no key registered → `OperatorPubKeyNotSet` (5033).

Because the message binds the transaction id, recipient, amount, currency and ledger
timestamp, an attestation cannot be replayed for a different mint, and a settled
`fintech_tx_id` cannot be minted twice (`DuplicateFintechTxId`, 5008).

Key custody is admin-gated and rotatable with `set_operator_pub_key` (admin `require_auth`);
rotating the key invalidates all previously issued attestations.

## 3. Security effect

- **Objective raised:** minting now needs *both* the Stellar operator key *and* the ed25519
  signing key. A single compromised backend key is no longer sufficient for unlimited minting.
- **Replay resistant:** the attestation is bound to the exact commitment and the settled
  `fintech_tx_id` is one-time.
- **Governed key lifecycle:** registration/rotation is admin-only and takes effect immediately.

## 4. Evidence — regression tests

The following tests in `acbu_minting/tests/test_mint_from_fiat.rs` prove the mitigation:

| Test | Proves |
| --- | --- |
| `test_w10_sec_004_operator_key_alone_cannot_mint` | The genuine Stellar operator key, without a valid attestation, cannot mint; total supply is unchanged. |
| `test_w10_sec_004_attestation_from_unregistered_key_rejected` | An attestation signed by an unrelated key is rejected (`#5032`). |
| `test_w10_sec_004_tampered_signature_rejected` | A corrupted signature under the correct `pub_key` is rejected. |
| `test_w10_sec_004_attestation_is_bound_to_commitment` | A valid attestation cannot be reused for a different amount. |
| `test_w10_sec_004_key_rotation_invalidates_prior_attestations` | Rotating the key rejects retired-key attestations and accepts new-key attestations. |
| `test_w10_sec_004_settled_attestation_cannot_be_replayed` | A settled fiat transaction id cannot be minted twice. |

Run them with:

```bash
cargo test -p acbu_minting --test test_mint_from_fiat
```

## 5. Residual risk and recommendations

The attestation key is still a single key controlled by the fintech operator. To further
reduce unilateral-control risk, the following remain recommended:

1. Hold `operator_pub_key` in an HSM or a dedicated signer that is **distinct** from the
   Stellar operator key (and ideally held by the fintech partner, not the backend).
2. Implement a **multisig / M-of-N threshold** mint authority for large mints, so no single
   attestation key can authorize unlimited supply by itself.
3. Monitor `1120 mint` / `op_pk` events for unexpected key rotations.

These are defense-in-depth extensions; the single-key unilateral-minting vector described
in W10-SEC-004 is closed by the dual-authorization requirement above.
