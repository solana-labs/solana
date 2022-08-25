import {hmac} from '@noble/hashes/hmac';
import {sha256} from '@noble/hashes/sha256';
import * as secp256k1 from '@noble/secp256k1';

// Supply a synchronous hashing algorithm to make this
// library interoperable with the synchronous APIs in web3.js.
secp256k1.utils.hmacSha256Sync = (key: Uint8Array, ...msgs: Uint8Array[]) => {
  const h = hmac.create(sha256, key);
  msgs.forEach(msg => h.update(msg));
  return h.digest();
};

export const ecdsaSign = (
  msgHash: Parameters<typeof secp256k1.signSync>[0],
  privKey: Parameters<typeof secp256k1.signSync>[1],
) => secp256k1.signSync(msgHash, privKey, {der: false, recovered: true});
export const isValidPrivateKey = secp256k1.utils.isValidPrivateKey;
export const publicKeyCreate = secp256k1.getPublicKey;
