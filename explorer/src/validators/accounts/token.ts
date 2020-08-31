import {
  StructType,
  number,
  optional,
  enums,
  any,
  boolean,
  string,
  array,
  pick,
  nullable,
} from "superstruct";
import { Pubkey } from "validators/pubkey";

export type TokenAccountType = StructType<typeof TokenAccountType>;
export const TokenAccountType = enums(["mint", "account", "multisig"]);

export type TokenAccountState = StructType<typeof AccountState>;
const AccountState = enums(["initialized", "uninitialized", "frozen"]);

const TokenAmount = pick({
  decimals: number(),
  uiAmount: number(),
  amount: string(),
});

export type TokenAccountInfo = StructType<typeof TokenAccountInfo>;
export const TokenAccountInfo = pick({
  mint: Pubkey,
  owner: Pubkey,
  tokenAmount: TokenAmount,
  delegate: optional(Pubkey),
  state: AccountState,
  isNative: boolean(),
  rentExemptReserve: optional(TokenAmount),
  delegatedAmount: optional(TokenAmount),
  closeAuthority: optional(Pubkey),
});

export type MintAccountInfo = StructType<typeof MintAccountInfo>;
export const MintAccountInfo = pick({
  mintAuthority: nullable(Pubkey),
  supply: string(),
  decimals: number(),
  isInitialized: boolean(),
  freezeAuthority: nullable(Pubkey),
});

export type MultisigAccountInfo = StructType<typeof MultisigAccountInfo>;
export const MultisigAccountInfo = pick({
  numRequiredSigners: number(),
  numValidSigners: number(),
  isInitialized: boolean(),
  signers: array(Pubkey),
});

export type TokenAccount = StructType<typeof TokenAccount>;
export const TokenAccount = pick({
  type: TokenAccountType,
  info: any(),
});
