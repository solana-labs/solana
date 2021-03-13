/* eslint-disable @typescript-eslint/no-redeclare */

import { Infer, number, nullable, enums, type } from "superstruct";
import { PublicKeyFromString } from "validators/pubkey";
import { BigNumFromString } from "validators/bignum";

export type StakeAccountType = Infer<typeof StakeAccountType>;
export const StakeAccountType = enums([
  "uninitialized",
  "initialized",
  "delegated",
  "rewardsPool",
]);

export type StakeMeta = Infer<typeof StakeMeta>;
export const StakeMeta = type({
  rentExemptReserve: BigNumFromString,
  authorized: type({
    staker: PublicKeyFromString,
    withdrawer: PublicKeyFromString,
  }),
  lockup: type({
    unixTimestamp: number(),
    epoch: number(),
    custodian: PublicKeyFromString,
  }),
});

export type StakeAccountInfo = Infer<typeof StakeAccountInfo>;
export const StakeAccountInfo = type({
  meta: StakeMeta,
  stake: nullable(
    type({
      delegation: type({
        voter: PublicKeyFromString,
        stake: BigNumFromString,
        activationEpoch: BigNumFromString,
        deactivationEpoch: BigNumFromString,
        warmupCooldownRate: number(),
      }),
      creditsObserved: number(),
    })
  ),
});

export type StakeAccount = Infer<typeof StakeAccount>;
export const StakeAccount = type({
  type: StakeAccountType,
  info: StakeAccountInfo,
});
