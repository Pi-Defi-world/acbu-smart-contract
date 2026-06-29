# Soroban contract error codes (C-054)

This file is generated from the `#[contracterror]` enums in the workspace. Run `python scripts/generate_error_codes.py` to refresh it, or `python scripts/generate_error_codes.py --check` to verify it stays in sync.

Clients map `invoke_contract` / simulation failures using the contract error `u32` code. Codes are stable per contract, so do not renumber them without a migration plan.

## `shared` - `ContractError`

| Code | Variant | Description |
| ---: | --- | --- |
| 1 | `Unauthorized` | The caller is not authorized for this action (e.g. not the admin/operator, or `require_auth` was not satisfied). |
| 2 | `Paused` | The contract is paused; state-changing operations are temporarily disabled. |
| 3 | `InvalidAmount` | The supplied amount is invalid (non-positive, or outside the allowed bounds such as min/max mint or burn limits). |
| 4 | `InvalidRate` | The exchange/conversion rate is invalid (e.g. zero, negative, or rejected by outlier/deviation checks). |
| 5 | `InsufficientReserves` | Reserves are insufficient to back the requested mint/operation against the configured collateralization requirement. |
| 6 | `RateLimitExceeded` | The per-window operation rate limit has been exceeded; retry later. |
| 7 | `InvalidCurrency` | The currency code is not recognized or not registered in the oracle/basket. |
| 8 | `OracleError` | A cross-contract call to the oracle failed or returned an unusable result (e.g. missing or stale rate). |
| 9 | `ReserveError` | A cross-contract call to the reserve tracker failed or returned an unusable result. |
| 10 | `InsufficientBalance` | The account's token balance is too low to complete the transfer/operation. |
| 11 | `InvalidRecipient` | The recipient address is invalid for this operation (e.g. a contract address where a classic account is required). |
| 12 | `InvalidVersion` | WASM upgrade rejected: `new_version` must be greater than the stored version. |
| 13 | `NoPendingAdmin` | No admin transfer is in progress, so `accept_admin` has nothing to claim. |
| 14 | `AdminTimelockNotElapsed` | The two-step admin transfer timelock has not yet elapsed; the pending admin must wait before calling `accept_admin`. |
| 15 | `NoPendingAdminToCancel` | No admin transfer is in progress, so `cancel_admin_transfer` has nothing to cancel. |
| 9999 | `Unknown` | Catch-all for an unexpected/unclassified failure. Should not occur in normal operation; treat as an internal error. |

## `shared / reentrancy guard` - `ReentrancyError`

| Code | Variant | Description |
| ---: | --- | --- |
| 6001 | `ReentrantCall` | reentrant call detected |

## `acbu_multisig` - `Error`

| Code | Variant | Description |
| ---: | --- | --- |
| 1 | `AlreadyInitialized` | multisig already initialized |
| 2 | `NotInitialized` | multisig not initialized |
| 3 | `Unauthorized` | unauthorized |
| 4 | `ProposalNotFound` | proposal not found |
| 5 | `AlreadyApproved` | proposal already approved |
| 6 | `AlreadyExecuted` | proposal already executed |
| 7 | `Expired` | proposal expired |
| 8 | `ThresholdNotMet` | approval threshold not met |
| 9 | `InvalidThreshold` | invalid threshold |
| 10 | `TooManySigners` | too many signers |
| 11 | `EmptySigners` | signers list cannot be empty |
| 12 | `DuplicateSigner` | duplicate signer |
| 999 | `Unknown` | unknown multisig error |

## `acbu_savings_vault` - `Error`

| Code | Variant | Description |
| ---: | --- | --- |
| 1001 | `Paused` | savings vault is paused |
| 1002 | `InvalidAmount` | invalid amount |
| 1003 | `NoDeposit` | no deposit found |
| 1004 | `AccountingError` | accounting error |
| 1005 | `Overflow` | overflow |
| 1006 | `InsufficientUnlocked` | insufficient unlocked balance |
| 1007 | `InvalidTerm` | invalid term |
| 1008 | `NotInitialized` | savings vault not initialized |
| 1009 | `NoAdmin` | no admin configured |
| 1010 | `AlreadyInitialized` | savings vault already initialized |
| 1011 | `InvalidFeeRate` | invalid fee rate |
| 1012 | `InvalidYieldRate` | invalid yield rate |
| 1013 | `NoFeeRate` | fee rate not configured |
| 1014 | `NoYieldRate` | yield rate not configured |
| 1015 | `ZeroNetDeposit` | net deposit is zero |
| 1016 | `InvalidVersion` | invalid contract version |
| 1017 | `TimelockNotElapsed` | timelock has not elapsed |
| 1018 | `NoPendingUpgrade` | no pending upgrade |
| 1019 | `NoPendingAdmin` | no pending admin |
| 1020 | `AdminTimelockNotElapsed` | admin timelock has not elapsed |
| 1021 | `NoPendingAdminToCancel` | no pending admin to cancel |
| 1999 | `Unknown` | unknown savings vault error |

## `acbu_lending_pool` - `Error`

| Code | Variant | Description |
| ---: | --- | --- |
| 1 | `NotFound` | resource not found |
| 2 | `InvalidState` | invalid lending pool state |
| 3 | `Unauthorized` | unauthorized |
| 4 | `AlreadyInitialized` | lending pool already initialized |
| 5 | `InvalidAmount` | invalid amount |
| 6 | `InsufficientBalance` | insufficient balance |
| 7 | `InsufficientCollateral` | insufficient collateral |
| 8 | `InsufficientLiquidity` | insufficient liquidity |
| 9 | `DustBalance` | dust balance |
| 2001 | `Paused` | lending pool is paused |
| 2002 | `InvalidVersion` | invalid contract version |
| 2003 | `TimelockNotElapsed` | timelock has not elapsed |
| 2004 | `NoPendingUpgrade` | no pending upgrade |
| 2005 | `NoPendingAdmin` | no pending admin |
| 2006 | `AdminTimelockNotElapsed` | admin timelock has not elapsed |
| 2007 | `NoPendingAdminToCancel` | no pending admin to cancel |
| 2999 | `Unknown` | unknown lending pool error |

## `acbu_escrow` - `EscrowError`

| Code | Variant | Description |
| ---: | --- | --- |
| 3001 | `Paused` | escrow is paused |
| 3002 | `InvalidAmount` | invalid escrow amount |
| 3003 | `EscrowNotFound` | escrow not found |
| 3004 | `PayerMismatch` | payer mismatch |
| 3005 | `EscrowExists` | escrow already exists |
| 3006 | `UninitializedAdmin` | escrow admin not initialized |
| 3007 | `UninitializedAcBuToken` | escrow token not initialized |
| 3008 | `AlreadyInitialized` | escrow already initialized |
| 3009 | `TimelockNotElapsed` | timelock has not elapsed |
| 3010 | `NoPendingUpgrade` | no pending upgrade |
| 3011 | `Unauthorized` | unauthorized |
| 3012 | `NoPendingAdmin` | no pending admin |
| 3013 | `AdminTimelockNotElapsed` | admin timelock has not elapsed |
| 3014 | `NoPendingAdminToCancel` | no pending admin to cancel |
| 3015 | `InsufficientBalance` | insufficient contract balance |
| 3016 | `Expired` | escrow has expired |
| 3017 | `SelfEscrow` | payee cannot be the same as payer |
| 3999 | `Unknown` | unknown escrow error |

## `acbu_minting` - `MintingError`

| Code | Variant | Description |
| ---: | --- | --- |
| 5001 | `AlreadyInitialized` | minting contract already initialized |
| 5002 | `InvalidFeeRate` | invalid fee rate |
| 5003 | `InvalidMintAmount` | invalid mint amount |
| 5004 | `InsufficientReserves` | insufficient reserves |
| 5005 | `ProofAlreadyUsed` | proof already used |
| 5006 | `InvalidOracleRate` | invalid oracle rate |
| 5007 | `UnauthorizedOperator` | unauthorized operator |
| 5008 | `DuplicateFintechTxId` | duplicate fintech transaction id |
| 5009 | `InvalidDripAmount` | invalid drip amount |
| 5010 | `DripExceedsCap` | drip exceeds cap |
| 5011 | `InsufficientDemoCustody` | insufficient demo custody |
| 5012 | `Paused` | minting contract is paused |
| 5013 | `OracleStale` | oracle rate is stale |
| 5014 | `FintechTxIdEmpty` | fintech transaction id is empty |
| 5015 | `FintechTxIdTooShort` | fintech transaction id is too short |
| 5016 | `FintechTxIdTooLong` | fintech transaction id is too long |
| 5017 | `FintechTxIdInvalidChar` | fintech transaction id contains invalid characters |
| 5018 | `InvalidVersion` | invalid contract version |
| 5019 | `MaxSupplyExceeded` | maximum supply exceeded |
| 5020 | `NoPendingAdmin` | no pending admin |
| 5021 | `AdminTimelockNotElapsed` | admin timelock has not elapsed |
| 5022 | `NoPendingAdminToCancel` | no pending admin to cancel |
| 5023 | `InvalidRecipient` | invalid recipient |
| 5024 | `InvalidRoleSeparation` | invalid role separation |
| 5999 | `Unknown` | unknown minting error |

## `acbu_oracle` - `OracleError`

| Code | Variant | Description |
| ---: | --- | --- |
| 7001 | `AlreadyInitialized` | oracle already initialized |
| 7002 | `InvalidMinSignatures` | invalid minimum signatures |
| 7003 | `MinSignaturesZero` | minimum signatures cannot be zero |
| 7004 | `NoPendingAdmin` | no pending admin |
| 7005 | `AdminTimelockNotElapsed` | admin timelock has not elapsed |
| 7006 | `NoPendingAdminToCancel` | no pending admin to cancel |
| 7007 | `UnauthorizedValidator` | unauthorized validator |
| 7008 | `UpdateIntervalNotMet` | update interval not met |
| 7009 | `InsufficientOracleSources` | insufficient oracle sources |
| 7010 | `InvalidRate` | invalid rate |
| 7011 | `RateNotFound` | rate not found |
| 7012 | `STokenNotConfigured` | s-token not configured |
| 7013 | `ValidatorAlreadyExists` | validator already exists |
| 7014 | `CannotRemoveValidator` | cannot remove validator |
| 7015 | `InvalidVersion` | invalid contract version |
| 7016 | `RateStaleLedger` | rate is stale |
| 7017 | `NoPendingUpgrade` | no pending upgrade |
| 7018 | `UpgradeTimelockNotElapsed` | upgrade timelock has not elapsed |
| 7019 | `NoPendingValidatorChange` | no pending validator change |
| 7020 | `ValidatorTimelockNotElapsed` | validator timelock has not elapsed |
| 7021 | `MaxValidatorsReached` | maximum validators reached |
| 7022 | `TimestampRollback` | timestamp rollback |
| 7023 | `RateNotInitialized` | rate not initialized - no submissions yet |
| 7999 | `Unknown` | unknown oracle error |

## `acbu_reserve_tracker` - `ReserveTrackerError`

| Code | Variant | Description |
| ---: | --- | --- |
| 8001 | `AlreadyInitialized` | reserve tracker already initialized |
| 8002 | `InvalidVersion` | invalid contract version |
| 8003 | `ZeroSupply` | zero supply |
| 8004 | `NoPendingAdmin` | no pending admin |
| 8005 | `AdminTimelockNotElapsed` | admin timelock has not elapsed |
| 8006 | `NoPendingAdminToCancel` | no pending admin to cancel |
| 8007 | `Unauthorized` | unauthorized |
| 8999 | `Unknown` | unknown reserve tracker error |
