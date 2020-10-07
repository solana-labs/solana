import { StructType, object, string, enums, pick } from "superstruct";
import { Pubkey } from "validators/pubkey";

export type NonceAccountType = StructType<typeof NonceAccountType>;
export const NonceAccountType = enums(["uninitialized", "initialized"]);

export type NonceAccountInfo = StructType<typeof NonceAccountInfo>;
export const NonceAccountInfo = pick({
  authority: Pubkey,
  blockhash: string(),
  feeCalculator: pick({
    lamportsPerSignature: string(),
  }),
});

export type NonceAccount = StructType<typeof NonceAccount>;
export const NonceAccount = object({
  type: NonceAccountType,
  info: NonceAccountInfo,
});
