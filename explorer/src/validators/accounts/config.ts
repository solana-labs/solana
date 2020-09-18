import {
  StructType,
  enums,
  pick,
  array,
  boolean,
  object,
  number,
  string,
  any,
  record,
} from "superstruct";

export type ConfigAccountType = StructType<typeof ConfigAccountType>;
export const ConfigAccountType = enums(["stakeConfig", "validatorInfo"]);

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

export type ConfigAccount = StructType<typeof ConfigAccount>;
export const ConfigAccount = object({
  type: ConfigAccountType,
  info: any(),
});
