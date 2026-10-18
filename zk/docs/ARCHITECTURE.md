# ZK Architecture

This document describes the zero-knowledge KYC verification architecture used
by the ACBU protocol.

## Overview

The client generates a ZK-enabled UltraHonk proof from private credential
inputs. The policy verifier checks its admin-configured policy and caller
binding, then calls a separate `zk_proof_verifier` Soroban contract to
cryptographically verify the proof against the configured verification key.

```text
Client
  Noir circuit + Barretenberg ZK backend
      | proof + five public field elements
      v
zk_verifier
  - checks caller-bound wallet hash and proof arguments
  - enforces stored minimum KYC tier and allowed country
  - checks KYC attestation and nullifier replay state
      | verify(proof, public_inputs)
      v
zk_proof_verifier
  - checks fixed proof/input lengths
  - verifies ZK-enabled UltraHonk proof using the stored VK
```

Soroban does not provide native BN254 or Poseidon2 precompiles; proof
verification is performed by the pinned verifier implementation in the
`zk_proof_verifier` contract.

## Public inputs and policy

The circuit exposes five 32-byte field elements in this order:

| Index | Public input | Contract enforcement |
|---|---|---|
| 0 | Credential commitment | Must match the submitted, attested commitment |
| 1 | `required_kyc` tier | Must be 1–3 and at least the stored policy minimum |
| 2 | `allowed_country` | Must be 1–999 and equal the stored policy country |
| 3 | Wallet-bound nullifier | Must match the submitted nullifier and not be spent |
| 4 | Wallet address hash | Must match the authenticated wallet's XDR hash |

The proof establishes that the private credential meets the public tier and
country assertions. Those assertions are not policy: the on-chain
`zk_verifier` requires an admin-configured policy and independently compares
the proved values against it. The proof-less legacy `verify` entrypoint fails
closed.

## Cryptographic primitives

| Primitive | Role | Where it runs |
|---|---|---|
| BN254 | Noir scalar field and proof system | Client prover and ZK proof verifier |
| Poseidon2 | Credential commitment and wallet-bound nullifier | Circuit and client |
| UltraHonk ZK | Zero-knowledge proof system | Barretenberg client and `zk_proof_verifier` |
| SHA-256 | Hash authenticated wallet XDR for caller binding | `zk_verifier` and client |

## Toolchain and verifier pinning

| Tool | Pinned version | Enforced by |
|---|---|---|
| Noir (`nargo`) | `1.0.0-beta.9` | Circuit `compiler_version` |
| Barretenberg (`bb`) | `v0.87.0` | `contracts/zk_verifier/README.md` |
| `@aztec/bb.js` | `0.87.0` | Frontend package manifest |
| `@noir-lang/*` | `1.0.0-beta.9` | Frontend package manifest |
| UltraZK Soroban verifier | `424ebf38ff73848cfedfe4f0a94431425916b866` | Cargo manifest and CI guard |

Proof generation and verification must use the ZK-enabled flavor (`keccakZK:
true` in the browser prover). The proof-verifier contract and its verification
key must be built for the matching Barretenberg serialization. Upgrade these
pins together and regenerate the key and proof artifacts atomically.

## Deployment and proof lifecycle

1. Compile the circuit and generate its ZK verification key with the pinned
   toolchain.
2. Deploy `zk_proof_verifier` with the administrator and verification key.
3. Deploy and initialize `zk_verifier`; set its KYC policy and link the
   deployed proof verifier with `set_proof_verifier`.
4. Register each approved credential commitment through the trusted KYC
   authority.
5. Generate a wallet-bound ZK proof and submit it to `verify_proof`.
6. Downstream contracts can query the resulting verification state through
   `is_verified` or the configured gate.

If either the policy or proof-verifier address is missing, verification fails
closed. The KYC policy and verification key are independently administered.
