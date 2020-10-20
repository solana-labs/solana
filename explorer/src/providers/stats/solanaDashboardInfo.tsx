import { EpochInfo, PerfSample } from "@solana/web3.js";
import { ClusterStatsStatus } from "./solanaClusterStats";

export type DashboardInfo = {
  status: ClusterStatsStatus;
  avgSlotTime_1h: number;
  avgSlotTime_1min: number;
  epochInfo: EpochInfo;
};

export enum DashboardInfoActionType {
  SetPerfSamples,
  SetEpochInfo,
  SetError,
  Reset,
}

export type DashboardInfoActionSetPerfSamples = {
  type: DashboardInfoActionType.SetPerfSamples;
  data: PerfSample[];
};

export type DashboardInfoActionSetEpochInfo = {
  type: DashboardInfoActionType.SetEpochInfo;
  data: EpochInfo;
};

export type DashboardInfoActionReset = {
  type: DashboardInfoActionType.Reset;
  data: DashboardInfo;
};

export type DashboardInfoActionSetError = {
  type: DashboardInfoActionType.SetError;
  data: string;
};

export type DashboardInfoAction =
  | DashboardInfoActionSetPerfSamples
  | DashboardInfoActionSetEpochInfo
  | DashboardInfoActionReset
  | DashboardInfoActionSetError;

export function dashboardInfoReducer(
  state: DashboardInfo,
  action: DashboardInfoAction
) {
  const status =
    state.avgSlotTime_1h !== 0 && state.epochInfo.absoluteSlot !== 0
      ? ClusterStatsStatus.Ready
      : ClusterStatsStatus.Loading;

  switch (action.type) {
    case DashboardInfoActionType.SetPerfSamples:
      if (action.data.length < 1) {
        return state;
      }

      const samples = action.data
        .map((sample) => {
          return sample.samplePeriodSecs / sample.numSlots;
        })
        .slice(0, 60);

      const samplesInHour = samples.length < 60 ? samples.length : 60;
      const avgSlotTime_1h =
        samples.reduce((sum: number, cur: number) => {
          return sum + cur;
        }, 0) / samplesInHour;

      return {
        ...state,
        avgSlotTime_1h,
        avgSlotTime_1min: samples[0],
        status,
      };
    case DashboardInfoActionType.SetEpochInfo:
      return {
        ...state,
        epochInfo: action.data,
        status,
      };
    case DashboardInfoActionType.SetError:
      return {
        ...state,
        status: ClusterStatsStatus.Error,
      };
    case DashboardInfoActionType.Reset:
      return {
        ...action.data,
      };
    default:
      return state;
  }
}
