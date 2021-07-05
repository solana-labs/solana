/* eslint-disable @typescript-eslint/no-redeclare */

import { Infer, string, enums, type } from "superstruct";
import { PublicKeyFromString } from "validators/pubkey";

export type NonceAccountType = Infer<typeof NonceAccountType>;
export const NonceAccountType = enums(["uninitialized", "initialized"]);

export type NonceAccountInfo = Infer<typeof NonceAccountInfo>;
export const NonceAccountInfo = type({
  authority: PublicKeyFromString,
  blockhash: string(),
  feeCalculator: type({
    lamportsPerSignature: string(),
  }),
});

export type NonceAccount = Infer<typeof NonceAccount>;
export const NonceAccount = type({
  type: NonceAccountType,
  info: NonceAccountInfo,
});
