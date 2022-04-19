import { expect } from "chai";
import { lamportsToSand, LAMPORTS_PER_SAND } from "utils";
import BN from "bn.js";

describe("lamportsToSand", () => {
  it("0 lamports", () => {
    expect(lamportsToSand(new BN(0))).to.eq(0.0);
  });

  it("1 lamport", () => {
    expect(lamportsToSand(new BN(1))).to.eq(0.000000001);
    expect(lamportsToSand(new BN(-1))).to.eq(-0.000000001);
  });

  it("1 SAND", () => {
    expect(lamportsToSand(new BN(LAMPORTS_PER_SAND))).to.eq(1.0);
    expect(lamportsToSand(new BN(-LAMPORTS_PER_SAND))).to.eq(-1.0);
  });

  it("u64::MAX lamports", () => {
    expect(lamportsToSand(new BN(2).pow(new BN(64)))).to.eq(
      18446744073.709551615
    );
    expect(lamportsToSand(new BN(2).pow(new BN(64)).neg())).to.eq(
      -18446744073.709551615
    );
  });
});
