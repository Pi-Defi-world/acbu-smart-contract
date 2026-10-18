/**
 * Poseidon2 hash helpers using @aztec/bb.js single-threaded WASM.
 *
 * These compute the same Poseidon2 values used by the Noir circuit.
 *
 * The frontend pre-computes commitment and nullifier before
 * feeding them as public inputs into the Noir prover.
 */

import { BarretenbergSync, Fr } from "@aztec/bb.js";

let bb: BarretenbergSync | null = null;

async function getBb(): Promise<BarretenbergSync> {
  if (!bb) {
    // bb.js 0.87.0 exposes a singleton constructor (`new` is private).
    bb = await BarretenbergSync.initSingleton();
  }
  return bb;
}

/**
 * Compute Poseidon2 hash of 2 field elements.
 */
export async function poseidon2Hash2(a: bigint, b: bigint): Promise<bigint> {
  const bb = await getBb();
  const fields = [new Fr(a), new Fr(b)];
  const hash = bb.poseidon2Hash(fields);
  return BigInt(hash.toString());
}

/**
 * Compute Poseidon2 hash of 4 field elements.
 */
export async function poseidon2Hash4(
  a: bigint,
  b: bigint,
  c: bigint,
  d: bigint
): Promise<bigint> {
  const bb = await getBb();
  const fields = [new Fr(a), new Fr(b), new Fr(c), new Fr(d)];
  const hash = bb.poseidon2Hash(fields);
  return BigInt(hash.toString());
}

/**
 * Compute the credential commitment from private data.
 */
export async function computeCommitment(cred: {
  kycLevel: number;
  countryCode: number;
  salt: bigint;
}): Promise<bigint> {
  return poseidon2Hash4(
    BigInt(cred.kycLevel),
    BigInt(cred.countryCode),
    cred.salt,
    0n
  );
}

/**
 * Compute a nullifier bound to a commitment, salt, and wallet hash.
 */
export async function computeNullifier(
  commitment: bigint,
  salt: bigint,
  walletAddressHash: bigint
): Promise<bigint> {
  return poseidon2Hash4(commitment, salt, walletAddressHash, 0n);
}

/**
 * Compute the attested credential binding hash (AZ-002).
 *
 * This is what the KYC authority attests alongside the commitment: the
 * circuit ties it to the same private credential via
 * `poseidon2_hash_2(kyc_level, country_code)`, so only credentials the
 * authority actually verified can be proven. The authority computes this
 * from its own verified records, never from user-claimed values.
 */
export async function computeAttestedCredential(cred: {
  kycLevel: number;
  countryCode: number;
}): Promise<bigint> {
  return poseidon2Hash2(BigInt(cred.kycLevel), BigInt(cred.countryCode));
}
