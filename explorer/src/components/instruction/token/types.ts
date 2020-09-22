import {
  enums,
  object,
  StructType,
  number,
  string,
  optional,
  array,
  pick,
  nullable,
  union,
} from "superstruct";
import { Pubkey } from "validators/pubkey";

const TokenAmountUi = object({
  amount: string(),
  decimals: number(),
  uiAmount: number(),
});

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
  amount: union([string(), number()]),
  authority: optional(Pubkey),
  multisigAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const Approve = object({
  source: Pubkey,
  delegate: Pubkey,
  amount: union([string(), number()]),
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
  amount: union([string(), number()]),
  mintAuthority: optional(Pubkey),
  multisigMintAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const Burn = object({
  account: Pubkey,
  mint: Pubkey,
  amount: union([string(), number()]),
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

const FreezeAccount = object({
  account: Pubkey,
  mint: Pubkey,
  freezeAuthority: optional(Pubkey),
  multisigOwner: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const ThawAccount = object({
  account: Pubkey,
  mint: Pubkey,
  freezeAuthority: optional(Pubkey),
  multisigOwner: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const Transfer2 = object({
  source: Pubkey,
  mint: Pubkey,
  destination: Pubkey,
  authority: optional(Pubkey),
  multisigAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
  tokenAmount: TokenAmountUi,
});

const Approve2 = object({
  source: Pubkey,
  mint: Pubkey,
  delegate: Pubkey,
  owner: optional(Pubkey),
  multisigOwner: optional(Pubkey),
  signers: optional(array(Pubkey)),
  tokenAmount: TokenAmountUi,
});

const MintTo2 = object({
  account: Pubkey,
  mint: Pubkey,
  mintAuthority: Pubkey,
  multisigMintAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
  tokenAmount: TokenAmountUi,
});

const Burn2 = object({
  account: Pubkey,
  mint: Pubkey,
  authority: optional(Pubkey),
  multisigAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
  tokenAmount: TokenAmountUi,
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
  "freezeAccount",
  "thawAccount",
  "transfer2",
  "approve2",
  "mintTo2",
  "burn2",
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
  freezeAccount: FreezeAccount,
  thawAccount: ThawAccount,
  transfer2: Transfer2,
  approve2: Approve2,
  mintTo2: MintTo2,
  burn2: Burn2,
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
  freezeAccount: "Freeze Account",
  thawAccount: "Thaw Account",
  transfer2: "Transfer 2",
  approve2: "Approve 2",
  mintTo2: "Mint To 2",
  burn2: "Burn 2",
};
