# acbu_escrow

Hold ACBU; release or refund by rules or dispute. Merchant/e-commerce settlements. Part of ACBU smart-contract-first protocols.

## Functions

- `initialize(admin, acbu_token)` — Initialize the escrow contract
- `create(payer, payee, amount, escrow_id)` — Create escrow (payer deposits ACBU)
- `release(escrow_id)` — Release to payee
- `refund(escrow_id, payer)` — Refund to payer; admin can authorize before expiry, and the payer can authorize after expiry
- `pause` / `unpause` — Admin only

## Events

- `EscrowCreatedEvent` — On create
- `EscrowReleasedEvent` — On release
- `EscrowRefundedEvent` — On refund
