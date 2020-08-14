import { object, StructType, number, nullable, enums } from "superstruct";
import { Pubkey } from "validators/pubkey";
import { BigNum } from "validators/bignum";

export type StakeAccountType = StructType<typeof StakeAccountType>;
export const StakeAccountType = enums([
  "uninitialized",
  "initialized",
  "delegated",
  "rewardsPool",
]);

export type StakeMeta = StructType<typeof StakeMeta>;
export const StakeMeta = object({
  rentExemptReserve: BigNum,
  authorized: object({
    staker: Pubkey,
    withdrawer: Pubkey,
  }),
  lockup: object({
    unixTimestamp: number(),
    epoch: number(),
    custodian: Pubkey,
  }),
});

export type StakeAccountInfo = StructType<typeof StakeAccountInfo>;
export const StakeAccountInfo = object({
  meta: StakeMeta,
  stake: nullable(
    object({
      delegation: object({
        voter: Pubkey,
        stake: BigNum,
        activationEpoch: BigNum,
        deactivationEpoch: BigNum,
        warmupCooldownRate: number(),
      }),
      creditsObserved: number(),
    })
  ),
});

export type StakeAccount = StructType<typeof StakeAccount>;
export const StakeAccount = object({
  type: StakeAccountType,
  info: StakeAccountInfo,
});
