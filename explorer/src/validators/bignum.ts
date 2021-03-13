import { coerce, instance, string } from "superstruct";
import BN from "bn.js";

export const BigNumFromString = coerce(instance(BN), string(), (value) => {
  if (typeof value === "string") return new BN(value, 10);
  throw new Error("invalid big num");
});
