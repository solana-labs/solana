import {
  StructType,
  pick,
  array,
  boolean,
  object,
  number,
  string,
  record,
  union,
  literal,
} from "superstruct";

export type StakeConfigInfo = StructType<typeof StakeConfigInfo>;
export const StakeConfigInfo = pick({
  warmupCooldownRate: number(),
  slashPenalty: number(),
});

export type ConfigKey = StructType<typeof ConfigKey>;
export const ConfigKey = pick({
  pubkey: string(),
  signer: boolean(),
});

export type ValidatorInfoConfigData = StructType<
  typeof ValidatorInfoConfigData
>;
export const ValidatorInfoConfigData = record(string(), string());

export type ValidatorInfoConfigInfo = StructType<
  typeof ValidatorInfoConfigInfo
>;
export const ValidatorInfoConfigInfo = pick({
  keys: array(ConfigKey),
  configData: ValidatorInfoConfigData,
});

export type ValidatorInfoAccount = StructType<typeof ValidatorInfoAccount>;
export const ValidatorInfoAccount = object({
  type: literal("validatorInfo"),
  info: ValidatorInfoConfigInfo,
});

export type StakeConfigInfoAccount = StructType<typeof StakeConfigInfoAccount>;
export const StakeConfigInfoAccount = object({
  type: literal("stakeConfig"),
  info: StakeConfigInfo,
});

export type ConfigAccount = StructType<typeof ConfigAccount>;
export const ConfigAccount = union([
  StakeConfigInfoAccount,
  ValidatorInfoAccount,
]);
