/* eslint-disable @typescript-eslint/no-redeclare */

import {
  Infer,
  array,
  boolean,
  type,
  number,
  string,
  record,
  union,
  literal,
} from "superstruct";

export type StakeConfigInfo = Infer<typeof StakeConfigInfo>;
export const StakeConfigInfo = type({
  warmupCooldownRate: number(),
  slashPenalty: number(),
});

export type ConfigKey = Infer<typeof ConfigKey>;
export const ConfigKey = type({
  pubkey: string(),
  signer: boolean(),
});

export type ValidatorInfoConfigData = Infer<typeof ValidatorInfoConfigData>;
export const ValidatorInfoConfigData = record(string(), string());

export type ValidatorInfoConfigInfo = Infer<typeof ValidatorInfoConfigInfo>;
export const ValidatorInfoConfigInfo = type({
  keys: array(ConfigKey),
  configData: ValidatorInfoConfigData,
});

export type ValidatorInfoAccount = Infer<typeof ValidatorInfoAccount>;
export const ValidatorInfoAccount = type({
  type: literal("validatorInfo"),
  info: ValidatorInfoConfigInfo,
});

export type StakeConfigInfoAccount = Infer<typeof StakeConfigInfoAccount>;
export const StakeConfigInfoAccount = type({
  type: literal("stakeConfig"),
  info: StakeConfigInfo,
});

export type ConfigAccount = Infer<typeof ConfigAccount>;
export const ConfigAccount = union([
  StakeConfigInfoAccount,
  ValidatorInfoAccount,
]);
