# Contracts – Known Issues

Issues found in the smart contracts (acbu_minting, acbu_burning, acbu_oracle, acbu_reserve_tracker, acbu_savings_vault, acbu_lending_pool, acbu_escrow, shared). Add new items below as numbered list entries.

---

## Critical

1. **Missing access control on escrow `release`** – acbu_escrow/src/lib.rs: `release` has no auth check. Any address can call `release(escrow_id)` and send funds to the payee.
2. **Unrestricted minting in `mint_from_fiat`** – acbu_minting/src/lib.rs: `mint_from_fiat` does not validate `fintech_tx_id` or off-chain fiat deposits. With `check_admin_or_user`, the recipient can call it for themselves and mint ACBU without real fiat backing.
3. **Escrow ID collision** – acbu_escrow/src/lib.rs: Escrow keys use only `(ESCROW, escrow_id)`. Two payers can use the same `escrow_id`; the second create overwrites the first. The first payer's funds become unrecoverable.
4. **Incorrect total supply in `verify_reserves`** – acbu_reserve_tracker/src/lib.rs: `verify_reserves` uses `acbu_client.balance(&env.current_contract_address())` as total supply. The reserve tracker does not hold ACBU, so this is always 0. The function returns `true` early and reserve checks never actually run.
5. **Missing auth checks in savings vault** – acbu_savings_vault/src/lib.rs: `deposit` and `withdraw` lack `user.require_auth()`. Anyone can call `withdraw(user=X, term_seconds=Y, amount=Z)` and move funds without X's authorization.
6. **Missing auth checks in lending pool** – acbu_lending_pool/src/lib.rs: `deposit` and `withdraw` lack `lender.require_auth()`. Anyone can call `withdraw(lender=X, amount=Y)` and drain X's balance.

## High

7. **Term not enforced in savings vault** – acbu_savings_vault/src/lib.rs: `term_seconds` is stored but never checked on withdrawal. Users can deposit and withdraw immediately; there is no lock period.
8. **No max amount check in `mint_from_fiat`** – acbu_minting/src/lib.rs: `mint_from_usdc` enforces `max_mint_amount`, but `mint_from_fiat` only checks `min_amount`, allowing unbounded minting.
9. **Integer division truncation in `burn_for_basket`** – acbu_burning/src/lib.rs: `amount_per_account = acbu_after_fee / (recipient_accounts.len() as i128)` truncates. With many recipients, dust is lost and never accounted for.
10. **No duplicate escrow check** – acbu_escrow/src/lib.rs: `create` does not check if `escrow_id` already exists. Overwriting an existing escrow can lock prior funds.
11. **Oracle and reserve tracker unused in minting** – acbu_minting/src/lib.rs: `oracle` and `reserve_tracker` are loaded but never called. Rates are hardcoded to 1:1 and reserve checks are skipped.
12. **Oracle and reserve tracker unused in burning** – acbu_burning/src/lib.rs: Same as minting — oracle and reserve tracker are loaded but not used; rates are hardcoded.

## Medium

13. **Token WASM import uses zero SHA256** – In acbu_minting, acbu_burning, and acbu_reserve_tracker, `soroban_token_contract.wasm` is imported with `sha256 = "0x0000...0"`. This is a placeholder; production builds should use the real WASM hash for integrity and security.
14. **Double `.unwrap().unwrap()` in escrow** – acbu_escrow/src/lib.rs: Multiple storage reads use `.get(...).unwrap().unwrap()`. Storage `get` returns `Option<T>`, so a single `.unwrap()` is expected. The second `.unwrap()` may panic or indicate a type mismatch.
15. **Double `.unwrap().unwrap()` in savings vault** – acbu_savings_vault/src/lib.rs: Same problematic double-unwrap pattern.
16. **Double `.unwrap().unwrap()` in lending pool** – acbu_lending_pool/src/lib.rs: Same pattern.
17. **Outlier detection has no effect in oracle** – acbu_oracle/src/lib.rs: Outliers are detected but only marked with a comment "Log outlier but continue with median". No logging, rejection, or alert occurs.
18. **Incorrect oracle test assertion** – acbu_oracle/tests/test.rs: Test expects `stored_rate == rate` (1234567), but the contract stores `median(sources)` (1235000). The test will fail.
19. **Redundant calculation in burning** – acbu_burning/src/lib.rs: `(acbu_after_fee * DECIMALS) / DECIMALS` is equivalent to `acbu_after_fee`; the multiplication and division cancel out.
20. **`median` uses `to_vec()` in no_std** – shared/src/lib.rs: `median` allocates with `values.to_vec()`. In `no_std` Soroban contracts this may not be available or may require `alloc`.
21. **Contract events vs backend listeners** – Verify that MintEvent and BurnEvent payloads (field names, types, decimals) match what the backend event listeners expect, to avoid parsing or indexing failures.

## Low

22. **Magic number for fee cap in savings and lending** – acbu_savings_vault and acbu_lending_pool use `10_000` instead of `shared::BASIS_POINTS`, unlike minting and burning.
23. **Incorrect fee in per-account BurnEvent** – acbu_burning/src/lib.rs: Each `BurnEvent` uses `calculate_fee(amount_per_account, fee_rate)`, but the fee is taken from the total `acbu_amount`. Per-account fees in events don't match actual fee accounting.
24. **`yield_amount` always 0 in savings** – acbu_savings_vault/src/lib.rs: `WithdrawEvent` always sets `yield_amount: 0`, suggesting yield logic is not implemented.
25. **Fee rate stored but unused in lending pool** – acbu_lending_pool/src/lib.rs: `fee_rate` is stored during initialization but never applied to any operation.
26. **Fee rate stored but unused in savings vault** – acbu_savings_vault/src/lib.rs: Same — `fee_rate` is stored but not applied.
27. **Loan events never emitted** – acbu_lending_pool/src/lib.rs: `LoanCreatedEvent` and `RepaymentEvent` are defined but never emitted; lending/repayment logic appears missing.
28. **No tests for core minting flows** – acbu_minting/tests/test.rs: Tests cover init, pause, and fee rate, but not `mint_from_usdc` or `mint_from_fiat`.

## Trivial

29. **Unused import** – acbu_minting/src/lib.rs: `Vec` is imported but not used.
30. **`len() == 0` style** – acbu_burning/src/lib.rs: `recipient_accounts.len() == 0` could be `recipient_accounts.is_empty()`.
31. **Empty validators in event** – acbu_oracle/src/lib.rs: `RateUpdateEvent` uses `validators: Vec::new(&env)` instead of the actual validator set.
32. **Weak `transaction_id` generation** – acbu_minting/src/lib.rs: `transaction_id` is `format!("mint_{}", ledger.sequence())`, which is predictable and not globally unique.
33. **No events in reserve tracker** – acbu_reserve_tracker/src/lib.rs: Reserve updates do not emit events, making off-chain tracking harder.
34. **No upgrade or versioning** – Multiple contracts: No version field or upgrade path; contracts cannot be migrated safely.
