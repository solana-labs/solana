/* eslint-disable @typescript-eslint/no-redeclare */

import { enums, number, type, string, Infer, optional } from "superstruct";
import { PublicKeyFromString } from "validators/pubkey";

export type InitializeBufferInfo = Infer<typeof InitializeBufferInfo>;
export const InitializeBufferInfo = type({
  account: PublicKeyFromString,
  authority: PublicKeyFromString,
});

export type WriteInfo = Infer<typeof WriteInfo>;
export const WriteInfo = type({
  offset: number(),
  bytes: string(),
  account: PublicKeyFromString,
  authority: PublicKeyFromString,
});

export type DeployWithMaxDataLenInfo = Infer<typeof DeployWithMaxDataLenInfo>;
export const DeployWithMaxDataLenInfo = type({
  maxDataLen: number(),
  payerAccount: PublicKeyFromString,
  programDataAccount: PublicKeyFromString,
  programAccount: PublicKeyFromString,
  bufferAccount: PublicKeyFromString,
  rentSysvar: PublicKeyFromString,
  clockSysvar: PublicKeyFromString,
  systemProgram: PublicKeyFromString,
  authority: PublicKeyFromString,
});

export type UpgradeInfo = Infer<typeof UpgradeInfo>;
export const UpgradeInfo = type({
  programDataAccount: PublicKeyFromString,
  programAccount: PublicKeyFromString,
  bufferAccount: PublicKeyFromString,
  spillAccount: PublicKeyFromString,
  rentSysvar: PublicKeyFromString,
  clockSysvar: PublicKeyFromString,
  authority: PublicKeyFromString,
});

export type SetAuthorityInfo = Infer<typeof SetAuthorityInfo>;
export const SetAuthorityInfo = type({
  account: PublicKeyFromString,
  authority: PublicKeyFromString,
  newAuthority: optional(PublicKeyFromString),
});

export type CloseInfo = Infer<typeof CloseInfo>;
export const CloseInfo = type({
  account: PublicKeyFromString,
  recipient: PublicKeyFromString,
  authority: PublicKeyFromString,
  programAccount: optional(PublicKeyFromString),
});

export type ExtendProgramInfo = Infer<typeof ExtendProgramInfo>;
export const ExtendProgramInfo = type({
  additionalBytes: number(),
  programDataAccount: PublicKeyFromString,
  programAccount: PublicKeyFromString,
  systemProgram: optional(PublicKeyFromString),
  payerAccount: optional(PublicKeyFromString),
});

export type UpgradeableBpfLoaderInstructionType = Infer<
  typeof UpgradeableBpfLoaderInstructionType
>;
export const UpgradeableBpfLoaderInstructionType = enums([
  "initializeBuffer",
  "write",
  "deployWithMaxDataLen",
  "upgrade",
  "setAuthority",
  "close",
  "extendProgram",
]);
