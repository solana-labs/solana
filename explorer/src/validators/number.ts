import { bigint, coerce, number, string } from "superstruct";

export const BigIntFromString = coerce(bigint(), string(), (value): bigint => {
  if (typeof value === "string") return BigInt(value);
  throw new Error("invalid bigint");
});

export const NumberFromString = coerce(number(), string(), (value): number => {
  if (typeof value === "string") return Number(value);
  throw new Error("invalid number");
});
