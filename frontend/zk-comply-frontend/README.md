# ZK-Comply Frontend (AZ-009)

Browser UI that:

1. Loads the Noir KYC-compliance circuit
2. Generates a wallet-bound, ZK-enabled UltraHonk proof via `generateProof`
3. Submits proof and public inputs to `zk_verifier.verify_proof` via `submitProof`

Before verification can succeed, deploy `zk_proof_verifier` with the circuit's
ZK verification key, deploy and configure `zk_verifier` with the authoritative
KYC policy, and link the proof-verifier address.

## Dev

```bash
cd frontend/zk-comply-frontend
npm install
npm run dev
```

Optional env:

- `VITE_VERIFIER_CONTRACT_ID` — deployed `zk_verifier` contract id
- `VITE_ACBU_API_URL` — ACBU backend base URL
