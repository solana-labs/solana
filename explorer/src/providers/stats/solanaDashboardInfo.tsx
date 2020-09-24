import { PerfSample } from "@solana/web3.js";

export type DashboardInfo = {
  loading: boolean;
  avgBlockTime_1h: number;
  avgBlockTime_1min: number;
  epochInfo: {
    absoluteSlot: number;
    blockHeight: number;
    epoch: number;
    slotIndex: number;
    slotsInEpoch: number;
  };
};

export enum DashboardInfoActionType {
  SetPerfSamples,
  SetEpochInfo,
  Reset,
}

export type DashboardInfoAction = {
  type: DashboardInfoActionType;
  data: {
    samples?: PerfSample[];
    epochInfo?: any;
    initialState?: DashboardInfo;
  };
};

export function dashboardInfoReducer(
  state: DashboardInfo,
  action: DashboardInfoAction
) {
  const loading =
    state.avgBlockTime_1h === 0 && state.epochInfo.absoluteSlot === 0;

  if (
    action.type === DashboardInfoActionType.SetPerfSamples &&
    action.data.samples
  ) {
    const samples = action.data.samples.map((sample) => {
      return sample.samplePeriodSecs / sample.numSlots;
    });

    let avgBlockTime_1h =
      samples.reduce((sum: number, cur: number) => {
        return sum + cur;
      }, 0) / 60;

    return {
      ...state,
      avgBlockTime_1h,
      avgBlockTime_1min: samples[0],
      loading,
    };
  }

  if (
    action.type === DashboardInfoActionType.SetEpochInfo &&
    action.data.epochInfo
  ) {
    return {
      ...state,
      epochInfo: action.data.epochInfo,
      loading,
    };
  }

  if (
    action.type === DashboardInfoActionType.Reset &&
    action.data.initialState
  ) {
    return {
      ...action.data.initialState,
    };
  }

  return {
    ...state,
    loading,
  };
}
