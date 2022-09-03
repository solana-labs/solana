import {sha512} from '@noble/hashes/sha512';
import * as ed25519 from '@noble/ed25519';

/**
 * A 64 byte secret key, the first 32 bytes of which is the
 * private scalar and the last 32 bytes is the public key.
 * Read more: https://blog.mozilla.org/warner/2011/11/29/ed25519-keys/
 */
type Ed25519SecretKey = Uint8Array;

/**
 * Ed25519 Keypair
 */
export interface Ed25519Keypair {
  publicKey: Uint8Array;
  secretKey: Ed25519SecretKey;
}

ed25519.utils.sha512Sync = (...m) => sha512(ed25519.utils.concatBytes(...m));

export const generatePrivateKey = ed25519.utils.randomPrivateKey;
export const generateKeypair = (): Ed25519Keypair => {
  const privateScalar = ed25519.utils.randomPrivateKey();
  const publicKey = getPublicKey(privateScalar);
  const secretKey = new Uint8Array(64);
  secretKey.set(privateScalar);
  secretKey.set(publicKey, 32);
  return {
    publicKey,
    secretKey,
  };
};
export const getPublicKey = ed25519.sync.getPublicKey;
export function isOnCurve(publicKey: Uint8Array): boolean {
  try {
    ed25519.Point.fromHex(publicKey, true /* strict */);
    return true;
  } catch {
    return false;
  }
}
export const sign = (
  message: Parameters<typeof ed25519.sync.sign>[0],
  secretKey: Ed25519SecretKey,
) => ed25519.sync.sign(message, secretKey.slice(0, 32));
export const verify = ed25519.sync.verify;
