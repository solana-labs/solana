import { waitReady, ed25519, hasher } from "./../";
import { performance } from "perf_hooks";
import * as crypto from "crypto";

// external references used for comparison
import nacl from "tweetnacl";
import { sha256 } from "crypto-hash";

const benchmark = (func: Function, iterations: number) => {
  /* any boilerplate code you want to have happen before the timer starts, perhaps copying a variable so it isn't mutated */
  const start = performance.now();
  for (let i = 0; i < iterations; i++) {
    func();
  }
  const finish = performance.now();
  return finish - start;
};

describe("Benchmark in Node.js WASM implementation of ED25519 vs tweetnacl", () => {
  beforeAll(async () => {
    await waitReady();
  });

  // on avg. WASM is 20-25x faster than tweetnacl but I am testing conservative assumption
  test("WASM sign method should be faster than nacl by at least 15x", async () => {
    const ITERATIONS = 100;
    const keypair = ed25519.keypair.fromSeed(
      Uint8Array.from(Array(32).fill(8))
    );
    const data = Uint8Array.from(crypto.randomBytes(1024));

    let wasm = benchmark(() => {
      ed25519.sign(keypair.publicKey, keypair.secretKey, data);
    }, ITERATIONS);
    let mod = benchmark(() => {
      nacl.sign.detached(data, keypair.secretKey);
    }, ITERATIONS);

    expect(mod / wasm).toBeGreaterThanOrEqual(15);
  });

  test("ed25519.sign should return the same signature as tweetnacl", async () => {
    const keypair = ed25519.keypair.fromSeed(
      Uint8Array.from(Array(32).fill(8))
    );
    const data = Uint8Array.from(crypto.randomBytes(1024));
    const actual = ed25519.sign(keypair.publicKey, keypair.secretKey, data);
    const expected = nacl.sign.detached(data, keypair.secretKey);

    expect(Buffer.from(actual).toString("base64")).toBe(
      Buffer.from(expected).toString("base64")
    );
  });

  test("ed25519.verify should return true for valid signature and false otherwise", async () => {
    const keypair = ed25519.keypair.fromSeed(
      Uint8Array.from(Array(32).fill(8))
    );

    const data = Uint8Array.from(crypto.randomBytes(1024));
    const signature = nacl.sign.detached(data, keypair.secretKey);

    const valid = ed25519.verify(keypair.publicKey, signature, data);
    expect(valid).toBe(true);

    const invalid = ed25519.verify(keypair.publicKey, new Uint8Array(), data);
    expect(invalid).toBe(false);
  });

  test("ed25519.isOnCurve should return true for random public key", async () => {
    const keypair = ed25519.keypair.fromSeed(
      Uint8Array.from(Array(32).fill(8))
    );
  });

  test("sha256 should return the same result as crypto-hash", async () => {
    // TODO
  });
});
