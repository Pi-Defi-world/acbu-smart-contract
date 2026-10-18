# `zk_verifier` — ZK KYC verification

The browser prover and on-chain verifier must use one pinned toolchain:

| Tool | Version |
|---|---|
| Noir (`nargo`) | `1.0.0-beta.9` |
| Barretenberg (`bb`) | `v0.87.0` |
| `@aztec/bb.js` | `0.87.0` |
| `@noir-lang/*` | `1.0.0-beta.9` |

Proofs must use the ZK-enabled UltraHonk flavor (`keccakZK: true` in the
browser prover, equivalent to `bb 0.87.0 --scheme ultra_honk
--oracle_hash keccak --zk`). The proof verifier accepts only this proof format.

## W10-SEC-002 — on-chain enforcement

`zk_verifier` delegates proof validation to the separate
`zk_proof_verifier` contract, then enforces its admin-configured minimum KYC
tier and exact allowed country against the cryptographically verified public
inputs. Until a policy and proof-verifier address are configured, verification
fails closed. The legacy proof-less `verify` entrypoint always rejects.

The circuit proves the credential commitment and tier/country constraints. Its
wallet-bound nullifier incorporates the public hash of the submitting
address. The contract checks that hash against the authenticated caller and
checks the proof's commitment and nullifier against the invocation arguments.

## Pinned proof-verifier dependency

The `zk_proof_verifier` contract depends on `ultrazk-soroban-verifier` from
`BuidlZone-Labs/zicket-contract` at commit
`424ebf38ff73848cfedfe4f0a94431425916b866`. The implementation includes the
UltraZK transcript, Libra sumcheck masking, and hiding-polynomial checks
required for ZK-enabled proofs. The proof-verifier contract uses Soroban SDK
26.0.1 independently of the rest of the workspace and is called through the
Soroban contract interface; SDK Rust types do not cross that boundary.

CI allows this single pinned Git source and rejects other Git dependencies or
revisions. The dependency commit and Noir/Barretenberg pins must be changed
together only after regenerating the verification key and proof fixtures.
