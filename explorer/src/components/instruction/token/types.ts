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

const InitializeAccount2 = type({
  account: PublicKeyFromString,
  mint: PublicKeyFromString,
  rentSysvar: PublicKeyFromString,
  owner: PublicKeyFromString,
});

const InitializeAccount3 = type({
  account: PublicKeyFromString,
  mint: PublicKeyFromString,
  owner: PublicKeyFromString,
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

const GetAccountDataSize = type({
  mint: PublicKeyFromString,
  extensionTypes: optional(array(string())),
});

const InitializeImmutableOwner = type({
  account: PublicKeyFromString,
});

const AmountToUiAmount = type({
  mint: PublicKeyFromString,
  amount: union([string(), number()]),
});

const UiAmountToAmount = type({
  mint: PublicKeyFromString,
  uiAmount: string(),
});

const InitializeMintCloseAuthority = type({
  mint: PublicKeyFromString,
  newAuthority: PublicKeyFromString,
});

const TransferFeeExtension = type({
  mint: PublicKeyFromString,
  transferFeeConfigAuthority: PublicKeyFromString,
  withdrawWitheldAuthority: PublicKeyFromString,
  transferFeeBasisPoints: number(),
  maximumFee: number(),
});

const DefaultAccountStateExtension = type({
  mint: PublicKeyFromString,
  accountState: string(),
  freezeAuthority: optional(PublicKeyFromString),
});

const Reallocate = type({
  account: PublicKeyFromString,
  payer: PublicKeyFromString,
  systemProgram: PublicKeyFromString,
  extensionTypes: array(string()),
});

const MemoTransferExtension = type({
  account: PublicKeyFromString,
  owner: optional(PublicKeyFromString),
  multisigOwner: optional(PublicKeyFromString),
  signers: optional(array(PublicKeyFromString)),
});

const CreateNativeMint = type({
  payer: PublicKeyFromString,
  nativeMint: PublicKeyFromString,
  systemProgram: PublicKeyFromString,
});

export type TokenInstructionType = Infer<typeof TokenInstructionType>;
export const TokenInstructionType = enums([
  "initializeMint",
  "initializeAccount",
  "initializeAccount2",
  "initializeAccount3",
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
  "getAccountDataSize",
  "initializeImmutableOwner",
  "amountToUiAmount",
  "uiAmountToAmount",
  "initializeMintCloseAuthority",
  "transferFeeExtension",
  "defaultAccountStateExtension",
  "reallocate",
  "memoTransferExtension",
  "createNativeMint",
]);

export const IX_STRUCTS = {
  initializeMint: InitializeMint,
  initializeAccount: InitializeAccount,
  initializeAccount2: InitializeAccount2,
  initializeAccount3: InitializeAccount3,
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
  getAccountDataSize: GetAccountDataSize,
  initializeImmutableOwner: InitializeImmutableOwner,
  amountToUiAmount: AmountToUiAmount,
  uiAmountToAmount: UiAmountToAmount,
  initializeMintCloseAuthority: InitializeMintCloseAuthority,
  transferFeeExtension: TransferFeeExtension,
  defaultAccountStateExtension: DefaultAccountStateExtension,
  reallocate: Reallocate,
  memoTransferExtension: MemoTransferExtension,
  createNativeMint: CreateNativeMint,
};

export const IX_TITLES = {
  initializeMint: "Initialize Mint",
  initializeAccount: "Initialize Account",
  initializeAccount2: "Initialize Account (2)",
  initializeAccount3: "Initialize Account (3)",
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
  getAccountDataSize: "Get Account Data Size",
  initializeImmutableOwner: "Initialize Immutable Owner",
  amountToUiAmount: "Amount To UiAmount",
  uiAmountToAmount: "UiAmount To Amount",
  initializeMintCloseAuthority: "Initialize Mint Close Authority",
  transferFeeExtension: "Transfer Fee Extension",
  defaultAccountStateExtension: "Default Account State Extension",
  reallocate: "Reallocate",
  memoTransferExtension: "Memo Transfer Extension",
  createNativeMint: "Create Native Mint",
};
