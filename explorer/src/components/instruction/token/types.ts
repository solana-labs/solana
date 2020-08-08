import {
  enums,
  object,
  StructType,
  number,
  optional,
  array,
} from "superstruct";
import { Pubkey } from "validators/pubkey";

const InitializeMint = object({
  mint: Pubkey,
  amount: number(),
  decimals: number(),
  owner: optional(Pubkey),
  account: optional(Pubkey),
});

const InitializeAccount = object({
  account: Pubkey,
  mint: Pubkey,
  owner: Pubkey,
});

const InitializeMultisig = object({
  multisig: Pubkey,
  signers: array(Pubkey),
  m: number(),
});

const Transfer = object({
  source: Pubkey,
  destination: Pubkey,
  amount: number(),
  authority: optional(Pubkey),
  multisigAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const Approve = object({
  source: Pubkey,
  delegate: Pubkey,
  amount: number(),
  owner: optional(Pubkey),
  multisigOwner: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const Revoke = object({
  source: Pubkey,
  owner: optional(Pubkey),
  multisigOwner: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const SetOwner = object({
  owned: Pubkey,
  newOwner: Pubkey,
  owner: optional(Pubkey),
  multisigOwner: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const MintTo = object({
  mint: Pubkey,
  account: Pubkey,
  amount: number(),
  owner: optional(Pubkey),
  multisigOwner: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const Burn = object({
  account: Pubkey,
  amount: number(),
  authority: optional(Pubkey),
  multisigAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const CloseAccount = object({
  account: Pubkey,
  destination: Pubkey,
  owner: optional(Pubkey),
  multisigOwner: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

export type TokenInstructionType = StructType<typeof TokenInstructionType>;
export const TokenInstructionType = enums([
  "initializeMint",
  "initializeAccount",
  "initializeMultisig",
  "transfer",
  "approve",
  "revoke",
  "setOwner",
  "mintTo",
  "burn",
  "closeAccount",
]);

export const IX_STRUCTS = {
  initializeMint: InitializeMint,
  initializeAccount: InitializeAccount,
  initializeMultisig: InitializeMultisig,
  transfer: Transfer,
  approve: Approve,
  revoke: Revoke,
  setOwner: SetOwner,
  mintTo: MintTo,
  burn: Burn,
  closeAccount: CloseAccount,
};
