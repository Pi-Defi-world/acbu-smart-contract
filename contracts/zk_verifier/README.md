# `zk_verifier` — proof artifact toolchain

The proof artifacts (UltraHonk proof + verification key + public inputs)
accepted by this contract and the `sdk`'s on-chain verifier flow must be
built with **one pinned toolchain**:

| Tool                        | Pinned version    |
|-----------------------------|------------------|
| Noir (`nargo`)              | `1.0.0-beta.9`    |
| Barretenberg (`bb`)         | `v0.87.0`        |
| `@aztec/bb.js` (JS binding) | `0.87.0`         |
| `@noir-lang/*` packages     | `1.0.0-beta.9`    |

This matches the toolchain of `ultrahonk-soroban-verifier` (see its
`tests/build_circuits.sh`) and the `ultrahonk_rust_verifier` crate the
proof format is designed around.

## AZ-006 — Canonical `ultrahonk_rust_verifier` revision

The `ultrahonk_rust_verifier` crate is not sourced from crates.io.  It is
currently NOT added as a direct Cargo dependency of this contract, because
the workspace supply-chain policy (W2-Z-020, enforced by
`.github/workflows/deps-guard.yml`) forbids git-sourced entries in the
committed `Cargo.lock`.  Proof verification is therefore implemented
natively in `src/lib.rc`.

**Canonical revision:** `2a73c4ba11f073f1797d90915ebbb9eb9d09f445`

If the policy is ever relaxed and the crate is re-added as a Cargo
dependency, every manifest in this workspace that declares the crate MUST
pin this exact commit:

```toml
ultrahonk_rust_verifier = {
    git = "https://github.com/noir-lang/ultrahonk-soroban-verifier",
    rev = "2a73c4ba11f073f1797d90915ebbb9eb9d09f445"
}
```

Using a different `rev` — or omitting `rev` entirely — causes the on-chain
VK/proof serialization to diverge from the reference implementation, leading
to silent verification failures in production.  The CI step
d"Enforce consistent ultrahonk_rust_verifier rev (AZ-006)"` in
`.github/workflows/deps-guard.yml` will reject any manifest that uses a
different revision.

## Why the pin matters (AZ-011)

UltraHonk proof/VK serialization and constraint semantics changed between
Noir `beta.9` and later betas (e.g. `beta.22`). Artifacts generated with a
different Noir/Barretenberg version than the one the verifier expects will
**not verify on-chain**, even though the circuit source is identical. Never
build the circuits with a different toolchain version; upgrade only by
changing the pin everywhere at once.

```bash
# Install the pinned toolchain
curl -L https://raw.githubusercontent.com/noir-lang/noirup/main/install | bash
noirup -v 1.0.0-beta.9

curl -L https://raw.githubusercontent.com/AztecProtocol/aztec-packages/master/barretenberg/cpp/installation/install | bash
bbup -v v0.87.0
```

## Verification expiry and revocation (W10-SEC-011)

Verification is not permanent. The contract stores a per-user expiry
timestamp alongside the verified flag, and authorized admins can revoke
verification at any time.

- `set_verified(user, valid_until`) records the verified flag together
with a validity window. A credential is considered valid only while
`valid_until >= current ledger timestamp; after that it expires automatically.
- `is_verified(user)` returns false once the window has lapsed, even if
the flag was never explicitly cleared.
- `revoke(user)e lets an authorized admin clear the verified flag and
expiry immediately (K-YC change, sanction event, or credential leak).
- Expiry and revocation are enforced on-chain in `src/lib.rs`; the proof
verification path must not bypass these checks.
