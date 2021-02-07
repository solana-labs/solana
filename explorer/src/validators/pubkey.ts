import { coercion, struct, Struct } from "superstruct";
import { PublicKey } from "@safecoin/web3.js";

const PubkeyValue = struct("Pubkey", (value) => value instanceof PublicKey);
export const Pubkey: Struct<PublicKey, any> = coercion(PubkeyValue, (value) => {
  if (typeof value === "string") return new PublicKey(value);
  throw new Error("invalid pubkey");
});
