# acbu_multisig — C-043 Emergency Multisig for Admin Operations

Implements an **M-of-N multisig guard** for all ACBU admin operations.

## Problem (C-043)

Every contract previously stored a single `Address` as `ADMIN`. A single compromised key gives an attacker full control: pause/unpause, fee changes, upgrades, reserve manipulation. The blast radius is catastrophic.

## Solution

A standalone `MultisigContract` holds the signer list and threshold. Each protected contract sets the **multisig contract address** as its `ADMIN`. Admin-only functions still call `admin.require_auth()` — Soroban's auth tree propagates the M-of-N approval automatically once a proposal has been approved and executed.

## Proposal Lifecycle

```
signer_A  →  propose("pause")          → proposal_id = 0  (approval count = 1)
signer_B  →  approve(0)                → approval count = 2
signer_C  →  execute(0)                → marked executed, ProposalExecutedEvent emitted
any       →  target_contract.pause()   → succeeds because multisig auth is in the tree
```

## Configuration

| Parameter   | Recommended (mainnet) |
|-------------|----------------------|
| `threshold` | 3                    |
| `signers`   | 5 (hardware wallets) |
| TTL         | 48 hours             |

## Tests

```bash
cargo test -p acbu_multisig
```

All tests verify the M-of-N acceptance check:
- `test_execute_2_of_3_succeeds` — 2-of-3 passes
- `test_execute_3_of_5_succeeds` — 3-of-5 passes
- `test_execute_below_threshold_panics` — 1-of-3 (threshold=2) rejects
- `test_execute_twice_panics` — replay protection
- `test_execute_expired_panics` — 48-hour TTL enforced
