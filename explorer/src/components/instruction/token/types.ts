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

export type TokenAmountUi = StructType<typeof TokenAmountUi>;
export const TokenAmountUi = object({
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
  multisigFreezeAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const ThawAccount = object({
  account: Pubkey,
  mint: Pubkey,
  freezeAuthority: optional(Pubkey),
  multisigFreezeAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
});

const TransferChecked = object({
  source: Pubkey,
  mint: Pubkey,
  destination: Pubkey,
  authority: optional(Pubkey),
  multisigAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
  tokenAmount: TokenAmountUi,
});

const ApproveChecked = object({
  source: Pubkey,
  mint: Pubkey,
  delegate: Pubkey,
  owner: optional(Pubkey),
  multisigOwner: optional(Pubkey),
  signers: optional(array(Pubkey)),
  tokenAmount: TokenAmountUi,
});

const MintToChecked = object({
  account: Pubkey,
  mint: Pubkey,
  mintAuthority: Pubkey,
  multisigMintAuthority: optional(Pubkey),
  signers: optional(array(Pubkey)),
  tokenAmount: TokenAmountUi,
});

const BurnChecked = object({
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
  "transferChecked",
  "approveChecked",
  "mintToChecked",
  "burnChecked",
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
  transfer2: TransferChecked,
  approve2: ApproveChecked,
  mintTo2: MintToChecked,
  burn2: BurnChecked,
  transferChecked: TransferChecked,
  approveChecked: ApproveChecked,
  mintToChecked: MintToChecked,
  burnChecked: BurnChecked,
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
  transfer2: "Transfer (Checked)",
  approve2: "Approve (Checked)",
  mintTo2: "Mint To (Checked)",
  burn2: "Burn (Checked)",
  transferChecked: "Transfer (Checked)",
  approveChecked: "Approve (Checked)",
  mintToChecked: "Mint To (Checked)",
  burnChecked: "Burn (Checked)",
};
