import { expect } from "chai";
import { lamportsToSol, LAMPORTS_PER_SOL } from "utils";
import BN from "bn.js";

describe("lamportsToSol", () => {
  it("0 lamports", () => {
    expect(lamportsToSol(new BN(0))).to.eq(0.0);
  });

  it("1 lamport", () => {
    expect(lamportsToSol(new BN(1))).to.eq(0.000000001);
    expect(lamportsToSol(new BN(-1))).to.eq(-0.000000001);
  });

  it("1 SOL", () => {
    expect(lamportsToSol(new BN(LAMPORTS_PER_SOL))).to.eq(1.0);
    expect(lamportsToSol(new BN(-LAMPORTS_PER_SOL))).to.eq(-1.0);
  });

  it("u64::MAX lamports", () => {
    expect(lamportsToSol(new BN(2).pow(new BN(64)))).to.eq(
      18446744073.709551615
    );
    expect(lamportsToSol(new BN(2).pow(new BN(64)).neg())).to.eq(
      -18446744073.709551615
    );
  });
});
