import { StructType, object, string, enums } from "superstruct";
import { Pubkey } from "validators/pubkey";

export type NonceAccountType = StructType<typeof NonceAccountType>;
export const NonceAccountType = enums([
  "initialized"
]);

export type NonceAccountInfo = StructType<typeof NonceAccountInfo>;
export const NonceAccountInfo = object({
  authority: Pubkey,
  blockhash: string(),
  feeCalculator: object({
    lamportsPerSignature: string() // this is coming through as a string? TODO: investigate.
  })
});

export type NonceAccount = StructType<typeof NonceAccount>;
export const NonceAccount = object({
  type: NonceAccountType,
  info: NonceAccountInfo
});
