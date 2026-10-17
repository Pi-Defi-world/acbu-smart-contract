# ACBU Multisig Quorum Matrix

**Issue:** W10-S-011  
**Status:** Authoritative — all other documents must reference this file rather than stating quorum values inline.

---

## Overview

The ACBU protocol uses a single `acbu_multisig` contract (M-of-N threshold signatures) as the `admin` for all protected contracts. The threshold required to execute a proposal depends on the **action category**: higher-impact or harder-to-reverse actions require a larger fraction of signers.

The multisig is deployed with **7 signers** for mainnet and **5 signers** for testnet. Thresholds below are expressed for the mainnet configuration; testnet equivalents are shown in parentheses.

---

## Canonical Quorum Matrix

| Action category | Affected contracts | Mainnet threshold | Testnet threshold | Notes |
|---|---|---|---|---|
| **Emergency pause / unpause** | Minting, Burning, Savings Vault, Lending Pool, Escrow | **3-of-7** | 2-of-5 | Intentionally low — must be reachable quickly during an incident even with partial signer availability |
| **Fee rate changes** | Minting (`set_fee_rate`), Burning (`set_fee_rate`) | **3-of-7** | 2-of-5 | Same bar as pause; time-sensitive for market conditions |
| **Operator / validator parameter changes** | Minting (`set_operator`), Oracle (`add_validator`, `remove_validator`) | **4-of-7** | 3-of-5 | Affects rate integrity and fiat mint authority |
| **Reserve ratio change** | Reserve Tracker (`set_min_reserve_ratio`) | **4-of-7** | 3-of-5 | Collateral policy change; material risk impact |
| **Dependency address rotation** | All contracts (`set_oracle`, `set_reserve_tracker`, etc.) | **4-of-7** | 3-of-5 | Redirecting a trusted dependency is equivalent to an upgrade in impact |
| **Contract upgrade** | All contracts (`upgrade`) | **5-of-7** | 4-of-5 | Code change; irreversible once executed |
| **Multisig config change** | Multisig itself (`propose_update_config`) | **5-of-7** | 4-of-5 | Self-governance; changing the guard requires the highest bar |

---

## Deployment values

Set these values when calling `initialize` on the multisig contract:

### Mainnet

```bash
--signers '["<s1>","<s2>","<s3>","<s4>","<s5>","<s6>","<s7>"]'
--threshold 3   # starting threshold; raised per-proposal category above
```

> The multisig threshold set at deployment is the **minimum default**. Each action category in the table above maps to a distinct `MultisigAction` variant; the threshold enforced is the one the signers have agreed upon for that variant.  
> In practice, use separate multisig instances or configure per-action minimum approvals in your deployment runbook.

### Testnet

```bash
--signers '["<s1>","<s2>","<s3>","<s4>","<s5>"]'
--threshold 2
```

### Oracle (`min_signatures`)

The oracle's `min_signatures` parameter controls how many independent validator submissions are required before a rate is accepted. This is separate from the governance multisig:

| Network | Validators | `min_signatures` |
|---|---|---|
| Mainnet | 7 | 4 |
| Testnet | 3 | 2 |

---

## Rationale

- **Pause at 3-of-7**: An incident may occur when key holders are in different time zones or temporarily unavailable. Three is the minimum that prevents a single-signer unilateral pause while still being reachable within minutes.
- **Parameter changes at 4-of-7**: A simple majority of signers (4 of 7 = 57%) must agree before any parameter affecting protocol economics or trusted addresses changes.
- **Upgrades and self-governance at 5-of-7**: Code and signer-set changes are the highest-risk operations. Requiring 5-of-7 (71%) ensures no small coalition can capture the protocol.

---

## References

- [`acbu_multisig/src/lib.rs`](../acbu_multisig/src/lib.rs) — contract implementation (M-of-N is generic; thresholds are deployment-time configuration)
- [`DEPLOYMENT.md`](../DEPLOYMENT.md) — deployment commands and environment variable reference
- [`docs/threat-model.md`](threat-model.md) — authorization invocation trees and trust assumptions
- [`docs/CONTRACTS.md`](CONTRACTS.md) — per-contract function reference
