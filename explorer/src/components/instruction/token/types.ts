import {
  enums,
  object,
  StructType,
  number,
  optional,
  array,
  pick,
  nullable,
} from "superstruct";
import { Pubkey } from "validators/pubkey";

const InitializeMint = pick({
  mint: Pubkey,
  decimals: number(),
  mintAuthority: Pubkey,
  rentSysvar: Pubkey,
  freezeAuthority: optional(Pubkey),
});

const InitializeAccount = pick({
  account: Pubkey,
  mint: Pubkey,
  owner: Pubkey,
  rentSysvar: Pubkey,
});

const InitializeMultisig = pick({
  multisig: Pubkey,
  rentSysvar: Pubkey,
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

const AuthorityType = enums([
  "mintTokens",
  "freezeAccount",
  "accountOwner",
  "closeAccount",
]);

const SetAuthority = object({
  mint: optional(Pubkey),
  account: optional(Pubkey),
  authorityType: AuthorityType,
  newAuthority: nullable(Pubkey),
  authority: optional(Pubkey),
  multisigAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const MintTo = object({
  mint: Pubkey,
  account: Pubkey,
  amount: number(),
  mintAuthority: optional(Pubkey),
  multisigMintAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const Burn = object({
  account: Pubkey,
  mint: Pubkey,
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
  "setAuthority",
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
  setAuthority: SetAuthority,
  mintTo: MintTo,
  burn: Burn,
  closeAccount: CloseAccount,
};

export const IX_TITLES = {
  initializeMint: "Initialize Mint",
  initializeAccount: "Initialize Account",
  initializeMultisig: "Initialize Multisig",
  transfer: "Transfer",
  approve: "Approve",
  revoke: "Revoke",
  setAuthority: "Set Authority",
  mintTo: "Mint To",
  burn: "Burn",
  closeAccount: "Close Account",
};
