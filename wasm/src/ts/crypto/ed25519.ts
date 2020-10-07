import { getWASM, waitReady } from './../wrapper';
export * as keypair from './keypair';

export async function sign(
  pubkey: Uint8Array,
  seckey: Uint8Array,
  message: Uint8Array,
): Promise<Uint8Array> {
  await waitReady();
  return getWASM().signEd25519(pubkey, seckey, message);
}

export async function verify(
  pubkey: Uint8Array,
  signature: Uint8Array,
  data: Uint8Array,
): Promise<boolean> {
  await waitReady();
  return getWASM().verifyEd25519(pubkey, signature, data);
}

export async function isOnCurve(pubkey: Uint8Array): Promise<boolean> {
  await waitReady();
  return getWASM().isOnCurveEd25519(pubkey);
}
