// external references used for comparison
import nacl from 'tweetnacl';
import {sha256} from 'crypto-hash';
import {performance} from 'perf_hooks';
import * as crypto from 'crypto';

import {waitReady, ed25519, hasher} from './../';

const toHex = (arrayBuffer: ArrayBuffer | SharedArrayBuffer) => {
  return Buffer.from(arrayBuffer).toString('hex');
};

const benchmark = (func: () => void, iterations: number) => {
  const start = performance.now();
  for (let i = 0; i < iterations; i++) {
    func();
  }
  const finish = performance.now();
  return finish - start;
};

describe('Benchmark in Node.js WASM implementation of ED25519 vs tweetnacl', () => {
  beforeAll(async () => {
    await waitReady();
  });

  // on avg. WASM is 20-25x faster than tweetnacl but I am testing conservative assumption
  test('WASM sign method should be faster than nacl by at least 15x', () => {
    const ITERATIONS = 100;
    const keypair = ed25519.keypair.fromSeed(
      Uint8Array.from(Array(32).fill(8)),
    );
    const data = Uint8Array.from(crypto.randomBytes(1024));

    const wasm = benchmark(() => {
      ed25519.sign(keypair.publicKey, keypair.secretKey, data);
    }, ITERATIONS);
    const mod = benchmark(() => {
      nacl.sign.detached(data, keypair.secretKey);
    }, ITERATIONS);

    expect(mod / wasm).toBeGreaterThanOrEqual(15);
  });
});

describe('Key generation', () => {
  beforeAll(async () => {
    await waitReady();
  });

  test('secret key should be 64bit and public key should be 32bit', () => {
    const keypair = ed25519.keypair.generate();
    // const keypair = ed25519.keypair.fromSeed(
    //   Uint8Array.from(Array(32).fill(8))
    // );

    expect(keypair.secretKey).toBeDefined();
    expect(keypair.secretKey.length).toBe(64);
    expect(keypair.publicKey).toBeDefined();
    expect(keypair.publicKey.length).toBe(32);
  });

  test('given seed should return valid public key', () => {
    const keypair = ed25519.keypair.fromSeed(
      Uint8Array.from(Array(32).fill(8)),
    );

    const privateKey =
      '08080808080808080808080808080808080808080808080808080808080808081398f62c6d1a457c51ba6a4b5f3dbd2f69fca93216218dc8997e416bd17d93ca';
    const publicKey =
      '1398f62c6d1a457c51ba6a4b5f3dbd2f69fca93216218dc8997e416bd17d93ca';

    expect(toHex(keypair.secretKey)).toBe(privateKey);
    expect(toHex(keypair.publicKey)).toBe(publicKey);
  });

  test('given secret key fromSecretKey should return valid public key', () => {
    const privateKey =
      '08080808080808080808080808080808080808080808080808080808080808081398f62c6d1a457c51ba6a4b5f3dbd2f69fca93216218dc8997e416bd17d93ca';
    const keypair = ed25519.keypair.fromSecretKey(
      Uint8Array.from(Buffer.from(privateKey, 'hex')),
    );

    const publicKey =
      '1398f62c6d1a457c51ba6a4b5f3dbd2f69fca93216218dc8997e416bd17d93ca';

    expect(toHex(keypair.secretKey)).toBe(privateKey);
    expect(toHex(keypair.publicKey)).toBe(publicKey);
  });
});

describe('WASM implementation of ED25519 should be backwards compatible with tweetnacl', () => {
  beforeAll(async () => {
    await waitReady();
  });

  test('ed25519.sign should return the same signature as tweetnacl', () => {
    const keypair = ed25519.keypair.fromSeed(
      Uint8Array.from(Array(32).fill(8)),
    );
    const data = Uint8Array.from(crypto.randomBytes(1024));
    const actual = ed25519.sign(keypair.publicKey, keypair.secretKey, data);
    const expected = nacl.sign.detached(data, keypair.secretKey);

    expect(Buffer.from(actual).toString('base64')).toBe(
      Buffer.from(expected).toString('base64'),
    );
  });

  test('ed25519.verify should return true for valid signature and false otherwise', () => {
    const keypair = ed25519.keypair.fromSeed(
      Uint8Array.from(Array(32).fill(8)),
    );

    const data = Uint8Array.from(crypto.randomBytes(1024));
    const signature = nacl.sign.detached(data, keypair.secretKey);

    const valid = ed25519.verify(keypair.publicKey, signature, data);
    expect(valid).toBe(true);

    const invalid = ed25519.verify(keypair.publicKey, new Uint8Array(), data);
    expect(invalid).toBe(false);
  });

  test('ed25519.isOnCurve should return true for random public key', () => {
    const keypair = ed25519.keypair.fromSeed(
      Uint8Array.from(Array(32).fill(8)),
    );

    expect(ed25519.isOnCurve(keypair.publicKey)).toBe(true);
  });

  test('sha256 should return the same result as crypto-hash', async () => {
    const data = Uint8Array.from(Array(32).fill(8));
    const expected = await sha256(data);
    expect(hasher.sha256(data)).toBe(expected);
  });
});
