import {
  StructType,
  enums,
  array,
  number,
  pick,
  object,
  boolean,
  string,
  any,
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
export const ClockAccountInfo = object({
  slot: number(),
  epoch: number(),
  leaderScheduleEpoch: number(),
  unixTimestamp: number(),
});

export type EpochScheduleInfo = StructType<typeof EpochScheduleInfo>;
export const EpochScheduleInfo = object({
  slotsPerEpoch: number(),
  leaderScheduleSlotOffset: number(),
  warmup: boolean(),
  firstNormalEpoch: number(),
  firstNormalSlot: number(),
});

export type FeesInfo = StructType<typeof FeesInfo>;
export const FeesInfo = object({
  feeCalculator: object({
    lamportsPerSignature: string(),
  }),
});

export type RecentBlockhashesEntry = StructType<typeof RecentBlockhashesEntry>;
export const RecentBlockhashesEntry = object({
  blockhash: string(),
  feeCalculator: object({
    lamportsPerSignature: string(),
  }),
});

export type RecentBlockhashesInfo = StructType<typeof RecentBlockhashesInfo>;
export const RecentBlockhashesInfo = array(RecentBlockhashesEntry);

export type RentInfo = StructType<typeof RentInfo>;
export const RentInfo = object({
  lamportsPerByteYear: string(),
  exemptionThreshold: number(),
  burnPercent: number(),
});

export type RewardsInfo = StructType<typeof RewardsInfo>;
export const RewardsInfo = object({
  validatorPointValue: number(),
});

export type SlotHashEntry = StructType<typeof SlotHashEntry>;
export const SlotHashEntry = object({
  slot: number(),
  hash: string(),
});

export type SlotHashesInfo = StructType<typeof SlotHashesInfo>;
export const SlotHashesInfo = array(SlotHashEntry);

export type SlotHistoryInfo = StructType<typeof SlotHistoryInfo>;
export const SlotHistoryInfo = object({
  nextSlot: number(),
  bits: string(),
});

export type StakeHistoryEntryItem = StructType<typeof StakeHistoryEntryItem>;
export const StakeHistoryEntryItem = object({
  effective: number(),
  activating: number(),
  deactivating: number(),
});

export type StakeHistoryEntry = StructType<typeof StakeHistoryEntry>;
export const StakeHistoryEntry = object({
  epoch: number(),
  stakeHistory: StakeHistoryEntryItem,
});

export type StakeHistoryInfo = StructType<typeof StakeHistoryInfo>;
export const StakeHistoryInfo = array(StakeHistoryEntry);

export type SysvarAccount = StructType<typeof SysvarAccount>;
export const SysvarAccount = object({
  type: SysvarAccountType,
  info: any(),
});
