import { StructType, enums, pick, array, boolean, object } from "superstruct";
import { Pubkey } from "validators/pubkey";

export type ConfigAccountType = StructType<typeof ConfigAccountType>;
export const ConfigAccountType = enums(["stakeConfig", "validatorInfo"]);

export type ConfigKey = StructType<typeof ConfigKey>;
export const ConfigKey = pick({
  pubkey: Pubkey,
  signer: boolean(),
});

export type ConfigAccountInfo = StructType<typeof ConfigAccountType>;
export const ConfigAccountInfo = pick({
  keys: array(ConfigKey),
});

export type ConfigAccount = StructType<typeof ConfigAccount>;
export const ConfigAccount = object({
  type: ConfigAccountType,
  info: ConfigAccountInfo,
});
