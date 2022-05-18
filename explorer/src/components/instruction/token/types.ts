/* eslint-disable @typescript-eslint/no-redeclare */

import {
  enums,
  type,
  Infer,
  number,
  string,
  optional,
  array,
  nullable,
  union,
} from "superstruct";
import { PublicKeyFromString } from "validators/pubkey";

export type TokenAmountUi = Infer<typeof TokenAmountUi>;
export const TokenAmountUi = type({
  amount: string(),
  decimals: number(),
  uiAmountString: string(),
});

const InitializeMint = type({
  mint: PublicKeyFromString,
  decimals: number(),
  mintAuthority: PublicKeyFromString,
  rentSysvar: PublicKeyFromString,
  freezeAuthority: optional(PublicKeyFromString),
});

const InitializeAccount = type({
  account: PublicKeyFromString,
  mint: PublicKeyFromString,
  owner: PublicKeyFromString,
  rentSysvar: PublicKeyFromString,
});

const InitializeMultisig = type({
  multisig: PublicKeyFromString,
  rentSysvar: PublicKeyFromString,
  signers: array(PublicKeyFromString),
  m: number(),
});

export type Transfer = Infer<typeof Transfer>;
export const Transfer = type({
  source: PublicKeyFromString,
  destination: PublicKeyFromString,
  amount: union([string(), number()]),
  authority: optional(PublicKeyFromString),
  multisigAuthority: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
});

const Approve = type({
  source: PublicKeyFromString,
  delegate: PublicKeyFromString,
  amount: union([string(), number()]),
  owner: optional(PublicKeyFromString),
  multisigOwner: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
});

const Revoke = type({
  source: PublicKeyFromString,
  owner: optional(PublicKeyFromString),
  multisigOwner: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
});

const AuthorityType = enums([
  "mintTokens",
  "freezeAccount",
  "accountOwner",
  "closeAccount",
]);

const SetAuthority = type({
  mint: optional(PublicKeyFromString),
  account: optional(PublicKeyFromString),
  authorityType: AuthorityType,
  newAuthority: nullable(PublicKeyFromString),
  authority: optional(PublicKeyFromString),
  multisigAuthority: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
});

const MintTo = type({
  mint: PublicKeyFromString,
  account: PublicKeyFromString,
  amount: union([string(), number()]),
  mintAuthority: optional(PublicKeyFromString),
  multisigMintAuthority: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
});

const Burn = type({
  account: PublicKeyFromString,
  mint: PublicKeyFromString,
  amount: union([string(), number()]),
  authority: optional(PublicKeyFromString),
  multisigAuthority: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
});

const CloseAccount = type({
  account: PublicKeyFromString,
  destination: PublicKeyFromString,
  owner: optional(PublicKeyFromString),
  multisigOwner: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
});

const FreezeAccount = type({
  account: PublicKeyFromString,
  mint: PublicKeyFromString,
  freezeAuthority: optional(PublicKeyFromString),
  multisigFreezeAuthority: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
});

const ThawAccount = type({
  account: PublicKeyFromString,
  mint: PublicKeyFromString,
  freezeAuthority: optional(PublicKeyFromString),
  multisigFreezeAuthority: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
});

export type TransferChecked = Infer<typeof TransferChecked>;
export const TransferChecked = type({
  source: PublicKeyFromString,
  mint: PublicKeyFromString,
  destination: PublicKeyFromString,
  authority: optional(PublicKeyFromString),
  multisigAuthority: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
  tokenAmount: TokenAmountUi,
});

const ApproveChecked = type({
  source: PublicKeyFromString,
  mint: PublicKeyFromString,
  delegate: PublicKeyFromString,
  owner: optional(PublicKeyFromString),
  multisigOwner: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
  tokenAmount: TokenAmountUi,
});

const MintToChecked = type({
  account: PublicKeyFromString,
  mint: PublicKeyFromString,
  mintAuthority: optional(PublicKeyFromString),
  multisigMintAuthority: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
  tokenAmount: TokenAmountUi,
});

const BurnChecked = type({
  account: PublicKeyFromString,
  mint: PublicKeyFromString,
  authority: optional(PublicKeyFromString),
  multisigAuthority: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
  tokenAmount: TokenAmountUi,
});

const SyncNative = type({
  account: PublicKeyFromString,
});

export type TokenInstructionType = Infer<typeof TokenInstructionType>;
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
  "syncNative",
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
  syncNative: SyncNative,
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
  syncNative: "Sync Native",
};
