import {
  StructType,
  enums,
  array,
  number,
  object,
  boolean,
  string,
  any,
  pick,
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

export type EpochScheduleInfo = StructType<typeof EpochScheduleInfo>;
export const EpochScheduleInfo = pick({
  slotsPerEpoch: number(),
  leaderScheduleSlotOffset: number(),
  warmup: boolean(),
  firstNormalEpoch: number(),
  firstNormalSlot: number(),
});

export type FeesInfo = StructType<typeof FeesInfo>;
export const FeesInfo = pick({
  feeCalculator: pick({
    lamportsPerSignature: string(),
  }),
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

export type RentInfo = StructType<typeof RentInfo>;
export const RentInfo = pick({
  lamportsPerByteYear: string(),
  exemptionThreshold: number(),
  burnPercent: number(),
});

export type RewardsInfo = StructType<typeof RewardsInfo>;
export const RewardsInfo = pick({
  validatorPointValue: number(),
});

export type SlotHashEntry = StructType<typeof SlotHashEntry>;
export const SlotHashEntry = pick({
  slot: number(),
  hash: string(),
});

export type SlotHashesInfo = StructType<typeof SlotHashesInfo>;
export const SlotHashesInfo = array(SlotHashEntry);

export type SlotHistoryInfo = StructType<typeof SlotHistoryInfo>;
export const SlotHistoryInfo = pick({
  nextSlot: number(),
  bits: string(),
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

export type SysvarAccount = StructType<typeof SysvarAccount>;
export const SysvarAccount = object({
  type: SysvarAccountType,
  info: any(),
});
