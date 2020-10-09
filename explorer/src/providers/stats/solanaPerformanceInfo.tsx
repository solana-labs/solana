import { PerfSample } from "@solana/web3.js";
import { ClusterStatsStatus } from "./solanaClusterStats";

export type PerformanceInfo = {
  status: ClusterStatsStatus;
  avgTps: number;
  historyMaxTps: number;
  perfHistory: {
    short: (number | null)[];
    medium: (number | null)[];
    long: (number | null)[];
  };
  transactionCount: number;
};

export enum PerformanceInfoActionType {
  SetTransactionCount,
  SetPerfSamples,
  SetError,
  Reset,
}

export type PerformanceInfoActionSetTransactionCount = {
  type: PerformanceInfoActionType.SetTransactionCount;
  data: number;
};

export type PerformanceInfoActionSetPerfSamples = {
  type: PerformanceInfoActionType.SetPerfSamples;
  data: PerfSample[];
};

export type PerformanceInfoActionSetError = {
  type: PerformanceInfoActionType.SetError;
  data: string;
};

export type PerformanceInfoActionReset = {
  type: PerformanceInfoActionType.Reset;
  data: PerformanceInfo;
};

export type PerformanceInfoAction =
  | PerformanceInfoActionSetTransactionCount
  | PerformanceInfoActionSetPerfSamples
  | PerformanceInfoActionSetError
  | PerformanceInfoActionReset;

export function performanceInfoReducer(
  state: PerformanceInfo,
  action: PerformanceInfoAction
) {
  const status =
    state.avgTps !== 0 && state.transactionCount !== 0
      ? ClusterStatsStatus.Ready
      : ClusterStatsStatus.Loading;

  switch (action.type) {
    case PerformanceInfoActionType.SetPerfSamples:
      if (action.data.length < 1) {
        return state;
      }

      let short = action.data.map((sample) => {
        return sample.numTransactions / sample.samplePeriodSecs;
      });

      const historyMaxTps = Math.max(...short);
      const avgTps = short[0];
      const medium = downsampleByFactor(short, 4);
      const long = downsampleByFactor(medium, 3);
      short = round(short.slice(0, 30)).reverse();

      const perfHistory = {
        short: short,
        medium: round(medium.slice(0, 30)).reverse(),
        long: round(long.slice(0, 30)).reverse(),
      };

      return {
        ...state,
        historyMaxTps,
        avgTps,
        perfHistory,
        status,
      };
    case PerformanceInfoActionType.SetTransactionCount:
      return {
        ...state,
        transactionCount: action.data,
        status,
      };
    case PerformanceInfoActionType.SetError:
      return {
        ...state,
        status: ClusterStatsStatus.Error,
      };
    case PerformanceInfoActionType.Reset:
      return {
        ...action.data,
      };
    default:
      return state;
  }
}

function downsampleByFactor(series: number[], factor: number) {
  return series.reduce((result: number[], num: number, i: number) => {
    const downsampledIndex = Math.floor(i / factor);
    if (result.length < downsampledIndex + 1) {
      result.push(0);
    }
    const mean = result[downsampledIndex];
    const differential = (num - mean) / ((i % factor) + 1);
    result[downsampledIndex] = mean + differential;
    return result;
  }, []);
}

function round(series: number[]) {
  return series.map((n) => Math.round(n));
}
