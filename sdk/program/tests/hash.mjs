import { expect } from "chai";
import { solana_program_init, Hash } from "crate";
solana_program_init();

// TODO: wasm_bindgen doesn't currently support exporting constants
const HASH_BYTES = 32;

describe("Hash", function () {
  it("invalid", () => {
    expect(() => {
      new Hash([
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0,
      ]);
    }).to.throw();

    expect(() => {
      new Hash([
        'invalid', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0,
      ]);
    }).to.throw();

    expect(() => {
      new Hash(
        "0x300000000000000000000000000000000000000000000000000000000000000000000"
      );
    }).to.throw();

    expect(() => {
      new Hash(
        "0x300000000000000000000000000000000000000000000000000000000000000"
      );
    }).to.throw();

    expect(() => {
      new Hash(
        "135693854574979916511997248057056142015550763280047535983739356259273198796800000"
      );
    }).to.throw();

    expect(() => {
      new Hash("12345");
    }).to.throw();
  });

  it("toString", () => {
    const key = new Hash("CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3");
    expect(key.toString()).to.eq("CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3");

    const key2 = new Hash("1111111111111111111111111111BukQL");
    expect(key2.toString()).to.eq("1111111111111111111111111111BukQL");

    const key3 = new Hash("11111111111111111111111111111111");
    expect(key3.toString()).to.eq("11111111111111111111111111111111");

    const key4 = new Hash([
      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      0, 0, 0, 0, 0, 0, 0,
    ]);
    expect(key4.toString()).to.eq("11111111111111111111111111111111");
  });

  it("toBytes", () => {
    const key = new Hash("CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3");
    expect(key.toBytes()).to.deep.equal(
      new Uint8Array([
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0,
      ])
    );

    const key2 = new Hash();
    expect(key2.toBytes()).to.deep.equal(
      new Uint8Array([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0,
      ])
    );
  });
});
