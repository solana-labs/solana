/* eslint-disable @typescript-eslint/no-redeclare */

import { enums, nullable, number, pick, string, StructType } from "superstruct";
import { Pubkey } from "validators/pubkey";

export type WriteInfo = StructType<typeof WriteInfo>;
export const WriteInfo = pick({
  account: Pubkey,
  authority: Pubkey,
  bytes: string(),
  offset: number(),
});

export type InitializeBufferInfo = StructType<typeof InitializeBufferInfo>;
export const InitializeBufferInfo = pick({
  account: Pubkey,
  authority: Pubkey,
});

export type UpgradeInfo = StructType<typeof UpgradeInfo>;
export const UpgradeInfo = pick({
  programDataAccount: Pubkey,
  programAccount: Pubkey,
  bufferAccount: Pubkey,
  spillAccount: Pubkey,
  authority: Pubkey,
  rentSysvar: Pubkey,
  clockSysvar: Pubkey,
});

export type SetAuthorityInfo = StructType<typeof SetAuthorityInfo>;
export const SetAuthorityInfo = pick({
  account: Pubkey,
  authority: Pubkey,
  newAuthority: nullable(Pubkey),
});

export type DeployWithMaxDataLenInfo = StructType<
  typeof DeployWithMaxDataLenInfo
>;
export const DeployWithMaxDataLenInfo = pick({
  programDataAccount: Pubkey,
  programAccount: Pubkey,
  payerAccount: Pubkey,
  bufferAccount: Pubkey,
  authority: Pubkey,
  rentSysvar: Pubkey,
  clockSysvar: Pubkey,
  systemProgram: Pubkey,
  maxDataLen: number(),
});

export type UpgradeableBpfLoaderInstructionType = StructType<
  typeof UpgradeableBpfLoaderInstructionType
>;
export const UpgradeableBpfLoaderInstructionType = enums([
  "initializeBuffer",
  "deployWithMaxDataLen",
  "setAuthority",
  "write",
  "finalize",
]);
