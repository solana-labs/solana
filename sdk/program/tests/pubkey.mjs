import { expect } from "chai";
import { solana_program_init, Pubkey } from "crate";
solana_program_init();

// TODO: wasm_bindgen doesn't currently support exporting constants
const MAX_SEED_LEN = 32;

describe("Pubkey", function () {
  it("invalid", () => {
    expect(() => {
      new Pubkey([
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0,
      ]);
    }).to.throw();

    expect(() => {
      new Pubkey([
        'invalid', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0,
      ]);
    }).to.throw();

    expect(() => {
      new Pubkey(
        "0x300000000000000000000000000000000000000000000000000000000000000000000"
      );
    }).to.throw();

    expect(() => {
      new Pubkey(
        "0x300000000000000000000000000000000000000000000000000000000000000"
      );
    }).to.throw();

    expect(() => {
      new Pubkey(
        "135693854574979916511997248057056142015550763280047535983739356259273198796800000"
      );
    }).to.throw();

    expect(() => {
      new Pubkey("12345");
    }).to.throw();
  });

  it("toString", () => {
    const key = new Pubkey("CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3");
    expect(key.toString()).to.eq("CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3");

    const key2 = new Pubkey("1111111111111111111111111111BukQL");
    expect(key2.toString()).to.eq("1111111111111111111111111111BukQL");

    const key3 = new Pubkey("11111111111111111111111111111111");
    expect(key3.toString()).to.eq("11111111111111111111111111111111");

    const key4 = new Pubkey([
      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      0, 0, 0, 0, 0, 0, 0,
    ]);
    expect(key4.toString()).to.eq("11111111111111111111111111111111");
  });

  it("toBytes", () => {
    const key = new Pubkey("CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3");
    expect(key.toBytes()).to.deep.equal(
      new Uint8Array([
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0,
      ])
    );

    const key2 = new Pubkey();
    expect(key2.toBytes()).to.deep.equal(
      new Uint8Array([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0,
      ])
    );
  });

  it("isOnCurve", () => {
    let onCurve = new Pubkey("J4NYrSRccTUGXP7wmFwiByakqWKZb5RwpiAoskpgAQRb");
    expect(onCurve.isOnCurve()).to.be.true;

    let offCurve = new Pubkey("12rqwuEgBYiGhBrDJStCiqEtzQpTTiZbh7teNVLuYcFA");
    expect(offCurve.isOnCurve()).to.be.false;
  });

  it("equals", () => {
    const arrayKey = new Pubkey([
      3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      0, 0, 0, 0, 0, 0, 0,
    ]);
    const base58Key = new Pubkey("CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3");

    expect(arrayKey.equals(base58Key)).to.be.true;
  });

  it("createWithSeed", async () => {
    const defaultPublicKey = new Pubkey("11111111111111111111111111111111");
    const derivedKey = Pubkey.createWithSeed(
      defaultPublicKey,
      "limber chicken: 4/45",
      defaultPublicKey
    );

    expect(
      derivedKey.equals(
        new Pubkey("9h1HyLCW5dZnBVap8C5egQ9Z6pHyjsh5MNy83iPqqRuq")
      )
    ).to.be.true;
  });

  it("createProgramAddress", async () => {
    const programId = new Pubkey("BPFLoader1111111111111111111111111111111111");
    const publicKey = new Pubkey("SeedPubey1111111111111111111111111111111111");

    let programAddress = Pubkey.createProgramAddress(
      [Buffer.from("", "utf8"), Buffer.from([1])],
      programId
    );
    expect(
      programAddress.equals(
        new Pubkey("3gF2KMe9KiC6FNVBmfg9i267aMPvK37FewCip4eGBFcT")
      )
    ).to.be.true;

    programAddress = Pubkey.createProgramAddress(
      [Buffer.from("☉", "utf8")],
      programId
    );
    expect(
      programAddress.equals(
        new Pubkey("7ytmC1nT1xY4RfxCV2ZgyA7UakC93do5ZdyhdF3EtPj7")
      )
    ).to.be.true;

    programAddress = Pubkey.createProgramAddress(
      [Buffer.from("Talking", "utf8"), Buffer.from("Squirrels", "utf8")],
      programId
    );
    expect(
      programAddress.equals(
        new Pubkey("HwRVBufQ4haG5XSgpspwKtNd3PC9GM9m1196uJW36vds")
      )
    ).to.be.true;

    programAddress = Pubkey.createProgramAddress(
      [publicKey.toBytes()],
      programId
    );
    expect(
      programAddress.equals(
        new Pubkey("GUs5qLUfsEHkcMB9T38vjr18ypEhRuNWiePW2LoK4E3K")
      )
    ).to.be.true;

    const programAddress2 = Pubkey.createProgramAddress(
      [Buffer.from("Talking", "utf8")],
      programId
    );
    expect(programAddress.equals(programAddress2)).to.eq(false);

    expect(() => {
      Pubkey.createProgramAddress([Buffer.alloc(MAX_SEED_LEN + 1)], programId);
    }).to.throw();
  });

  it("findProgramAddress", async () => {
    const programId = new Pubkey("BPFLoader1111111111111111111111111111111111");
    let [programAddress, nonce] = Pubkey.findProgramAddress(
      [Buffer.from("", "utf8")],
      programId
    );
    expect(
      programAddress.equals(
        Pubkey.createProgramAddress(
          [Buffer.from("", "utf8"), Buffer.from([nonce])],
          programId
        )
      )
    ).to.be.true;
  });
});
