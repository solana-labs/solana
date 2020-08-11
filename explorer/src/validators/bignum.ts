import { coercion, struct, Struct } from "superstruct";
import BN from "bn.js";

const BigNumValue = struct("BigNum", (value) => value instanceof BN);
export const BigNum: Struct<BN, any> = coercion(BigNumValue, (value) => {
  if (typeof value === "string") return new BN(value, 10);
  throw new Error("invalid big num");
});
