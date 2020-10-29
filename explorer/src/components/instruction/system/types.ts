/* eslint-disable @typescript-eslint/no-redeclare */

import { enums, number, pick, string, StructType } from "superstruct";
import { Pubkey } from "validators/pubkey";

export type CreateAccountInfo = StructType<typeof CreateAccountInfo>;
export const CreateAccountInfo = pick({
  source: Pubkey,
  newAccount: Pubkey,
  lamports: number(),
  space: number(),
  owner: Pubkey,
});

export type AssignInfo = StructType<typeof AssignInfo>;
export const AssignInfo = pick({
  account: Pubkey,
  owner: Pubkey,
});

export type TransferInfo = StructType<typeof TransferInfo>;
export const TransferInfo = pick({
  source: Pubkey,
  destination: Pubkey,
  lamports: number(),
});

export type CreateAccountWithSeedInfo = StructType<
  typeof CreateAccountWithSeedInfo
>;
export const CreateAccountWithSeedInfo = pick({
  source: Pubkey,
  newAccount: Pubkey,
  base: Pubkey,
  seed: string(),
  lamports: number(),
  space: number(),
  owner: Pubkey,
});

export type AdvanceNonceInfo = StructType<typeof AdvanceNonceInfo>;
export const AdvanceNonceInfo = pick({
  nonceAccount: Pubkey,
  nonceAuthority: Pubkey,
});

export type WithdrawNonceInfo = StructType<typeof WithdrawNonceInfo>;
export const WithdrawNonceInfo = pick({
  nonceAccount: Pubkey,
  destination: Pubkey,
  nonceAuthority: Pubkey,
  lamports: number(),
});

export type InitializeNonceInfo = StructType<typeof InitializeNonceInfo>;
export const InitializeNonceInfo = pick({
  nonceAccount: Pubkey,
  nonceAuthority: Pubkey,
});

export type AuthorizeNonceInfo = StructType<typeof AuthorizeNonceInfo>;
export const AuthorizeNonceInfo = pick({
  nonceAccount: Pubkey,
  nonceAuthority: Pubkey,
  newAuthorized: Pubkey,
});

export type AllocateInfo = StructType<typeof AllocateInfo>;
export const AllocateInfo = pick({
  account: Pubkey,
  space: number(),
});

export type AllocateWithSeedInfo = StructType<typeof AllocateWithSeedInfo>;
export const AllocateWithSeedInfo = pick({
  account: Pubkey,
  base: Pubkey,
  seed: string(),
  space: number(),
  owner: Pubkey,
});

export type AssignWithSeedInfo = StructType<typeof AssignWithSeedInfo>;
export const AssignWithSeedInfo = pick({
  account: Pubkey,
  base: Pubkey,
  seed: string(),
  owner: Pubkey,
});

export type TransferWithSeedInfo = StructType<typeof TransferWithSeedInfo>;
export const TransferWithSeedInfo = pick({
  source: Pubkey,
  sourceBase: Pubkey,
  destination: Pubkey,
  lamports: number(),
  sourceSeed: string(),
  sourceOwner: Pubkey,
});

export type SystemInstructionType = StructType<typeof SystemInstructionType>;
export const SystemInstructionType = enums([
  "createAccount",
  "createAccountWithSeed",
  "allocate",
  "allocateWithSeed",
  "assign",
  "assignWithSeed",
  "transfer",
  "advanceNonce",
  "withdrawNonce",
  "authorizeNonce",
  "initializeNonce",
  // "transferWithSeed", TODO: Add support for transfer with seed
]);
