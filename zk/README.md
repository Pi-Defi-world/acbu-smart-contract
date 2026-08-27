# ZK Circuits

This directory contains **Noir** circuits for the ACBU protocol. Circuits prove
compliance constraints (KYC tier, country restrictions, daily caps) without
revealing the underlying private data (KYC score).

## Circuits

| Package          | Location                       | Description                                      |
|------------------|--------------------------------|--------------------------------------------------|
| `kyc_verifier`   | `circuits/kyc_verifier/`       | Proves KYC tier ≥ `min_tier`, country allow-list |

## Prerequisites

Install [Nargo](https://noir-lang.org/docs/getting_started/installation/):

```bash
# Linux / macOS
curl -sSL https://github.com/noir-lang/noir/releases/download/v0.38.0/nargo-x86_64-unknown-linux-gnu.tar.gz \
  | tar -xz -C /usr/local/bin
```

## Running circuit tests

```bash
# Run all tests for the KYC verifier circuit
cd zk/circuits/kyc_verifier
nargo test --show-output
```

CI runs these automatically on every PR that modifies `zk/**` via
`.github/workflows/circuit-tests.yml`.

## How KYC proofs work

```
             ┌─────────────────────────────────┐
  private     │  kyc_score (e.g. 75)            │
  inputs  ──► │  tier      (derived = 2)         │  Noir circuit
             │                                   │  (compiled to
  public  ──► │  min_tier  (e.g. 2)              │   an R1CS /
  inputs     │  country_code  (e.g. 566 = NG)   │   ACIR)
             │  requested_amount                 │
             │  daily_cap                        │       │
             │  already_used                     │       │  prove
             └─────────────────────────────────┘       ▼
                                                   Proof π
                                                       │
                                                       │  verify
                                                       ▼
                                              on-chain verifier
                                              (future integration)
```

The circuit enforces:

1. `kyc_score` is in `[0, 100]`.
2. `tier` is correctly derived from `kyc_score`.
3. `tier >= min_tier`.
4. `requested_amount > 0`.
5. Country is on the Tier-1 allow-list (if `tier == 1`).
6. `already_used + requested_amount <= daily_cap`.

Closes issue **#651** (W2-Z-007).
