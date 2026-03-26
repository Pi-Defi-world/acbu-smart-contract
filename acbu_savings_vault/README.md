# acbu_savings_vault

Lock ACBU for fixed/rolling terms; yield accrual. Part of ACBU smart-contract-first protocols.

## Functions

- `initialize(admin, acbu_token, fee_rate_bps, yield_rate_bps)` — Initialize the vault
- `deposit(user, amount, term_seconds)` — Lock ACBU for a term
- `withdraw(user, term_seconds, amount)` — Withdraw principal (minus fee) plus accrued yield
- `get_balance(user, term_seconds)` — Get locked balance
- `pause` / `unpause` — Admin only

## Yield

- Yield accrues continuously from each deposit timestamp.
- Yield uses simple APR prorated by elapsed time.
- Formula: `yield = principal * yield_rate_bps * elapsed_seconds / (10_000 * 31_536_000)`.
- Withdraw payout to user is `(principal - fee) + yield`, and fee is sent to admin.

## Events

- `DepositEvent` — On deposit
- `WithdrawEvent` — On withdraw
