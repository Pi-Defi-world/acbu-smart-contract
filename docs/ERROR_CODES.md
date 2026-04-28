# Soroban contract error codes (C-054)

Clients map `invoke_contract` / simulation failures using the **contract error** `u32` code. Each ACBU contract exposes a dedicated `#[contracterror]` enum; codes are **stable per contract** (do not renumber without a migration plan).

The authoritative machine-readable list is emitted in each contract’s **Soroban JSON spec** (`stellar contract build` / `soroban contract bindings json`). This file is the human-readable index.

## `shared` — `ContractError` (burning contract)

| Code | Variant |
| ---: | --- |
| 1 | `Unauthorized` |
| 2 | `Paused` |
| 3 | `InvalidAmount` |
| 4 | `InvalidRate` |
| 5 | `InsufficientReserves` |
| 6 | `RateLimitExceeded` |
| 7 | `InvalidCurrency` |
| 8 | `OracleError` |
| 9 | `ReserveError` |
| 10 | `InsufficientBalance` |
| 11 | `InvalidRecipient` |
| 12 | `InvalidVersion` |

## `acbu_escrow` — `EscrowError`

| Code | Variant |
| ---: | --- |
| 3001 | `Paused` |
| 3002 | `InvalidAmount` |
| 3003 | `EscrowNotFound` |
| 3004 | `PayerMismatch` |
| 3005 | `EscrowExists` |
| 3006 | `UninitializedAdmin` |
| 3007 | `UninitializedAcBuToken` |
| 3008 | `AlreadyInitialized` |

## `acbu_savings_vault` — `Error`

| Code | Variant |
| ---: | --- |
| 1001 | `Paused` |
| 1002 | `InvalidAmount` |
| 1003 | `NoDeposit` |
| 1004 | `AccountingError` |
| 1005 | `Overflow` |
| 1006 | `InsufficientUnlocked` |
| 1007 | `InvalidTerm` |
| 1008 | `NotInitialized` |
| 1009 | `NoAdmin` |
| 1010 | `AlreadyInitialized` |
| 1011 | `InvalidFeeRate` |
| 1012 | `InvalidYieldRate` |
| 1013 | `NoFeeRate` |
| 1014 | `NoYieldRate` |
| 1015 | `ZeroNetDeposit` |
| 1016 | `InvalidVersion` |

## `acbu_lending_pool` — `Error`

| Code | Variant |
| ---: | --- |
| 4001 | `NotFound` |
| 4002 | `InvalidState` |
| 4003 | `Unauthorized` |
| 4004 | `AlreadyInitialized` |
| 4005 | `InvalidAmount` |
| 4006 | `Paused` |
| 4007 | `InsufficientBalance` |
| 4008 | `InvalidVersion` |

## `acbu_minting` — `MintingError`

| Code | Variant |
| ---: | --- |
| 5001 | `AlreadyInitialized` |
| 5002 | `InvalidFeeRate` |
| 5003 | `InvalidMintAmount` |
| 5004 | `InsufficientReserves` |
| 5005 | `ProofAlreadyUsed` |
| 5006 | `InvalidOracleRate` |
| 5007 | `UnauthorizedOperator` |
| 5008 | `DuplicateFintechTxId` |
| 5009 | `InvalidDripAmount` |
| 5010 | `DripExceedsCap` |
| 5011 | `InsufficientDemoCustody` |
| 5012 | `Paused` |
| 5013 | `OracleStale` |
| 5014 | `FintechTxIdEmpty` |
| 5015 | `FintechTxIdTooShort` |
| 5016 | `FintechTxIdTooLong` |
| 5017 | `FintechTxIdInvalidChar` |
| 5018 | `InvalidVersion` |

## `acbu_oracle` — `OracleError`

| Code | Variant |
| ---: | --- |
| 7001 | `AlreadyInitialized` |
| 7002 | `InvalidMinSignatures` |
| 7003 | `MinSignaturesZero` |
| 7004 | `NoPendingAdmin` |
| 7005 | `AdminTimelockNotElapsed` |
| 7006 | `NoPendingAdminToCancel` |
| 7007 | `UnauthorizedValidator` |
| 7008 | `UpdateIntervalNotMet` |
| 7009 | `InsufficientOracleSources` |
| 7010 | `InvalidRate` |
| 7011 | `RateNotFound` |
| 7012 | `STokenNotConfigured` |
| 7013 | `ValidatorAlreadyExists` |
| 7014 | `CannotRemoveValidator` |
| 7015 | `InvalidVersion` |
| 7016 | `RateStaleLedger` |

## `acbu_reserve_tracker` — `ReserveTrackerError`

| Code | Variant |
| ---: | --- |
| 8001 | `AlreadyInitialized` |
| 8002 | `InvalidVersion` |
