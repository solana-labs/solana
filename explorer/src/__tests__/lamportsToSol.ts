import { expect } from "chai";
import { lamportsToSafe, LAMPORTS_PER_SAFE } from "utils";
import BN from "bn.js";

describe("lamportsToSafe", () => {
  it("0 lamports", () => {
    expect(lamportsToSafe(new BN(0))).to.eq(0.0);
  });

  it("1 lamport", () => {
    expect(lamportsToSafe(new BN(1))).to.eq(0.000000001);
    expect(lamportsToSafe(new BN(-1))).to.eq(-0.000000001);
  });

  it("1 SAFE", () => {
    expect(lamportsToSafe(new BN(LAMPORTS_PER_SAFE))).to.eq(1.0);
    expect(lamportsToSafe(new BN(-LAMPORTS_PER_SAFE))).to.eq(-1.0);
  });

  it("u64::MAX lamports", () => {
    expect(lamportsToSafe(new BN(2).pow(new BN(64)))).to.eq(
      18446744073.709551615
    );
    expect(lamportsToSafe(new BN(2).pow(new BN(64)).neg())).to.eq(
      -18446744073.709551615
    );
  });
});
