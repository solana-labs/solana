/* eslint-disable @typescript-eslint/no-redeclare */

import {
  Infer,
  enums,
  array,
  number,
  type,
  boolean,
  string,
  literal,
  union,
} from "superstruct";

export type SysvarAccountType = Infer<typeof SysvarAccountType>;
export const SysvarAccountType = enums([
  "clock",
  "epochSchedule",
  "fees",
  "recentBlockhashes",
  "rent",
  "rewards",
  "slotHashes",
  "slotHistory",
  "stakeHistory",
]);

export type ClockAccountInfo = Infer<typeof ClockAccountInfo>;
export const ClockAccountInfo = type({
  slot: number(),
  epoch: number(),
  leaderScheduleEpoch: number(),
  unixTimestamp: number(),
});

export type SysvarClockAccount = Infer<typeof SysvarClockAccount>;
export const SysvarClockAccount = type({
  type: literal("clock"),
  info: ClockAccountInfo,
});

export type EpochScheduleInfo = Infer<typeof EpochScheduleInfo>;
export const EpochScheduleInfo = type({
  slotsPerEpoch: number(),
  leaderScheduleSlotOffset: number(),
  warmup: boolean(),
  firstNormalEpoch: number(),
  firstNormalSlot: number(),
});

export type SysvarEpochScheduleAccount = Infer<
  typeof SysvarEpochScheduleAccount
>;
export const SysvarEpochScheduleAccount = type({
  type: literal("epochSchedule"),
  info: EpochScheduleInfo,
});

export type FeesInfo = Infer<typeof FeesInfo>;
export const FeesInfo = type({
  feeCalculator: type({
    lamportsPerSignature: string(),
  }),
});

export type SysvarFeesAccount = Infer<typeof SysvarFeesAccount>;
export const SysvarFeesAccount = type({
  type: literal("fees"),
  info: FeesInfo,
});

export type RecentBlockhashesEntry = Infer<typeof RecentBlockhashesEntry>;
export const RecentBlockhashesEntry = type({
  blockhash: string(),
  feeCalculator: type({
    lamportsPerSignature: string(),
  }),
});

export type RecentBlockhashesInfo = Infer<typeof RecentBlockhashesInfo>;
export const RecentBlockhashesInfo = array(RecentBlockhashesEntry);

export type SysvarRecentBlockhashesAccount = Infer<
  typeof SysvarRecentBlockhashesAccount
>;
export const SysvarRecentBlockhashesAccount = type({
  type: literal("recentBlockhashes"),
  info: RecentBlockhashesInfo,
});

export type RentInfo = Infer<typeof RentInfo>;
export const RentInfo = type({
  lamportsPerByteYear: string(),
  exemptionThreshold: number(),
  burnPercent: number(),
});

export type SysvarRentAccount = Infer<typeof SysvarRentAccount>;
export const SysvarRentAccount = type({
  type: literal("rent"),
  info: RentInfo,
});

export type RewardsInfo = Infer<typeof RewardsInfo>;
export const RewardsInfo = type({
  validatorPointValue: number(),
});

export type SysvarRewardsAccount = Infer<typeof SysvarRewardsAccount>;
export const SysvarRewardsAccount = type({
  type: literal("rewards"),
  info: RewardsInfo,
});

export type SlotHashEntry = Infer<typeof SlotHashEntry>;
export const SlotHashEntry = type({
  slot: number(),
  hash: string(),
});

export type SlotHashesInfo = Infer<typeof SlotHashesInfo>;
export const SlotHashesInfo = array(SlotHashEntry);

export type SysvarSlotHashesAccount = Infer<typeof SysvarSlotHashesAccount>;
export const SysvarSlotHashesAccount = type({
  type: literal("slotHashes"),
  info: SlotHashesInfo,
});

export type SlotHistoryInfo = Infer<typeof SlotHistoryInfo>;
export const SlotHistoryInfo = type({
  nextSlot: number(),
  bits: string(),
});

export type SysvarSlotHistoryAccount = Infer<typeof SysvarSlotHistoryAccount>;
export const SysvarSlotHistoryAccount = type({
  type: literal("slotHistory"),
  info: SlotHistoryInfo,
});

export type StakeHistoryEntryItem = Infer<typeof StakeHistoryEntryItem>;
export const StakeHistoryEntryItem = type({
  effective: number(),
  activating: number(),
  deactivating: number(),
});

export type StakeHistoryEntry = Infer<typeof StakeHistoryEntry>;
export const StakeHistoryEntry = type({
  epoch: number(),
  stakeHistory: StakeHistoryEntryItem,
});

export type StakeHistoryInfo = Infer<typeof StakeHistoryInfo>;
export const StakeHistoryInfo = array(StakeHistoryEntry);

export type SysvarStakeHistoryAccount = Infer<typeof SysvarStakeHistoryAccount>;
export const SysvarStakeHistoryAccount = type({
  type: literal("stakeHistory"),
  info: StakeHistoryInfo,
});

export type SysvarAccount = Infer<typeof SysvarAccount>;
export const SysvarAccount = union([
  SysvarClockAccount,
  SysvarEpochScheduleAccount,
  SysvarFeesAccount,
  SysvarRecentBlockhashesAccount,
  SysvarRentAccount,
  SysvarRewardsAccount,
  SysvarSlotHashesAccount,
  SysvarSlotHistoryAccount,
  SysvarStakeHistoryAccount,
]);
