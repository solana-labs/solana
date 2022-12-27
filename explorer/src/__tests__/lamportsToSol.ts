import { expect } from "chai";
import { lamportsToSol, LAMPORTS_PER_SOL } from "utils";

describe("lamportsToSol", () => {
  it("0 lamports", () => {
    expect(lamportsToSol(0)).to.eq(0.0);
    expect(lamportsToSol(BigInt(0))).to.eq(0.0);
  });

  it("1 lamport", () => {
    expect(lamportsToSol(1)).to.eq(0.000000001);
    expect(lamportsToSol(BigInt(1))).to.eq(0.000000001);
    expect(lamportsToSol(-1)).to.eq(-0.000000001);
    expect(lamportsToSol(BigInt(-1))).to.eq(-0.000000001);
  });

  it("1 SOL", () => {
    expect(lamportsToSol(LAMPORTS_PER_SOL)).to.eq(1.0);
    expect(lamportsToSol(BigInt(LAMPORTS_PER_SOL))).to.eq(1.0);
    expect(lamportsToSol(-LAMPORTS_PER_SOL)).to.eq(-1.0);
    expect(lamportsToSol(BigInt(-LAMPORTS_PER_SOL))).to.eq(-1.0);
  });

  it("u64::MAX lamports", () => {
    expect(lamportsToSol(2n ** 64n)).to.eq(18446744073.709551615);
    expect(lamportsToSol(-(2n ** 64n))).to.eq(-18446744073.709551615);
  });
});
