import {
  StructType,
  enums,
  array,
  number,
  object,
  boolean,
  string,
  pick,
  literal,
  union,
} from "superstruct";

export type SysvarAccountType = StructType<typeof SysvarAccountType>;
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

export type ClockAccountInfo = StructType<typeof ClockAccountInfo>;
export const ClockAccountInfo = pick({
  slot: number(),
  epoch: number(),
  leaderScheduleEpoch: number(),
  unixTimestamp: number(),
});

export type SysvarClockAccount = StructType<typeof SysvarClockAccount>;
export const SysvarClockAccount = object({
  type: literal("clock"),
  info: ClockAccountInfo,
});

export type EpochScheduleInfo = StructType<typeof EpochScheduleInfo>;
export const EpochScheduleInfo = pick({
  slotsPerEpoch: number(),
  leaderScheduleSlotOffset: number(),
  warmup: boolean(),
  firstNormalEpoch: number(),
  firstNormalSlot: number(),
});

export type SysvarEpochScheduleAccount = StructType<
  typeof SysvarEpochScheduleAccount
>;
export const SysvarEpochScheduleAccount = object({
  type: literal("epochSchedule"),
  info: EpochScheduleInfo,
});

export type FeesInfo = StructType<typeof FeesInfo>;
export const FeesInfo = pick({
  feeCalculator: pick({
    lamportsPerSignature: string(),
  }),
});

export type SysvarFeesAccount = StructType<typeof SysvarFeesAccount>;
export const SysvarFeesAccount = object({
  type: literal("fees"),
  info: FeesInfo,
});

export type RecentBlockhashesEntry = StructType<typeof RecentBlockhashesEntry>;
export const RecentBlockhashesEntry = pick({
  blockhash: string(),
  feeCalculator: pick({
    lamportsPerSignature: string(),
  }),
});

export type RecentBlockhashesInfo = StructType<typeof RecentBlockhashesInfo>;
export const RecentBlockhashesInfo = array(RecentBlockhashesEntry);

export type SysvarRecentBlockhashesAccount = StructType<
  typeof SysvarRecentBlockhashesAccount
>;
export const SysvarRecentBlockhashesAccount = object({
  type: literal("recentBlockhashes"),
  info: RecentBlockhashesInfo,
});

export type RentInfo = StructType<typeof RentInfo>;
export const RentInfo = pick({
  lamportsPerByteYear: string(),
  exemptionThreshold: number(),
  burnPercent: number(),
});

export type SysvarRentAccount = StructType<typeof SysvarRentAccount>;
export const SysvarRentAccount = object({
  type: literal("rent"),
  info: RentInfo,
});

export type RewardsInfo = StructType<typeof RewardsInfo>;
export const RewardsInfo = pick({
  validatorPointValue: number(),
});

export type SysvarRewardsAccount = StructType<typeof SysvarRewardsAccount>;
export const SysvarRewardsAccount = object({
  type: literal("rewards"),
  info: RewardsInfo,
});

export type SlotHashEntry = StructType<typeof SlotHashEntry>;
export const SlotHashEntry = pick({
  slot: number(),
  hash: string(),
});

export type SlotHashesInfo = StructType<typeof SlotHashesInfo>;
export const SlotHashesInfo = array(SlotHashEntry);

export type SysvarSlotHashesAccount = StructType<
  typeof SysvarSlotHashesAccount
>;
export const SysvarSlotHashesAccount = object({
  type: literal("slotHashes"),
  info: SlotHashesInfo,
});

export type SlotHistoryInfo = StructType<typeof SlotHistoryInfo>;
export const SlotHistoryInfo = pick({
  nextSlot: number(),
  bits: string(),
});

export type SysvarSlotHistoryAccount = StructType<
  typeof SysvarSlotHistoryAccount
>;
export const SysvarSlotHistoryAccount = object({
  type: literal("slotHistory"),
  info: SlotHistoryInfo,
});

export type StakeHistoryEntryItem = StructType<typeof StakeHistoryEntryItem>;
export const StakeHistoryEntryItem = pick({
  effective: number(),
  activating: number(),
  deactivating: number(),
});

export type StakeHistoryEntry = StructType<typeof StakeHistoryEntry>;
export const StakeHistoryEntry = pick({
  epoch: number(),
  stakeHistory: StakeHistoryEntryItem,
});

export type StakeHistoryInfo = StructType<typeof StakeHistoryInfo>;
export const StakeHistoryInfo = array(StakeHistoryEntry);

export type SysvarStakeHistoryAccount = StructType<
  typeof SysvarStakeHistoryAccount
>;
export const SysvarStakeHistoryAccount = object({
  type: literal("stakeHistory"),
  info: StakeHistoryInfo,
});

export type SysvarAccount = StructType<typeof SysvarAccount>;
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
