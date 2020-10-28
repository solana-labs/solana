/* eslint-disable @typescript-eslint/no-redeclare */

import { enums, number, pick, string, StructType } from "superstruct";
import { Pubkey } from "validators/pubkey";

const CreateAccount = pick({
  source: Pubkey,
  newAccount: Pubkey,
  lamports: number(),
  space: number(),
  owner: Pubkey,
});

const Assign = pick({
  account: Pubkey,
  owner: Pubkey,
});

const Transfer = pick({
  source: Pubkey,
  destination: Pubkey,
  lamports: number(),
});

const CreateAccountWithSeed = pick({
  source: Pubkey,
  newAccount: Pubkey,
  base: Pubkey,
  seed: string(),
  lamports: number(),
  space: number(),
  owner: Pubkey,
});

const AdvanceNonceAccount = pick({
  nonceAccount: Pubkey,
  nonceAuthority: Pubkey,
});

const WithdrawNonceAccount = pick({
  nonceAccount: Pubkey,
  destination: Pubkey,
  nonceAuthority: Pubkey,
  lamports: number(),
});

const InitializeNonceAccount = pick({
  nonceAccount: Pubkey,
  nonceAuthority: Pubkey,
});

const AuthorizeNonceAccount = pick({
  nonceAccount: Pubkey,
  nonceAuthority: Pubkey,
  newAuthorized: Pubkey,
});

const Allocate = pick({
  account: Pubkey,
  space: number(),
});

const AllocateWithSeed = pick({
  account: Pubkey,
  base: Pubkey,
  seed: string(),
  space: number(),
  owner: Pubkey,
});

const AssignWithSeed = pick({
  account: Pubkey,
  base: Pubkey,
  seed: string(),
  owner: Pubkey,
});

const TransferWithSeed = pick({
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
  "advanceNonceAccount",
  "withdrawNonceAccount",
  "authorizeNonceAccount",
  "initializeNonceAccount",
]);

export const IX_STRUCTS: { [id: string]: any } = {
  createAccount: CreateAccount,
  createAccountWithSeed: CreateAccountWithSeed,
  allocate: Allocate,
  allocateWithSeed: AllocateWithSeed,
  assign: Assign,
  assignWithSeed: AssignWithSeed,
  transfer: Transfer,
  advanceNonceAccount: AdvanceNonceAccount,
  withdrawNonceAccount: WithdrawNonceAccount,
  authorizeNonceAccount: AuthorizeNonceAccount,
  initializeNonceAccount: InitializeNonceAccount,
  transferWithSeed: TransferWithSeed, // TODO: Add support for transfer with seed
};
