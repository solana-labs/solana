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
  SetErroredOut,
  Reset,
}

export type PerformanceInfoAction = {
  type: PerformanceInfoActionType;
  data: {
    samples?: PerfSample[];
    transactionCount?: number;
    initialState?: PerformanceInfo;
  };
};

export function performanceInfoReducer(
  state: PerformanceInfo,
  action: PerformanceInfoAction
) {
  const status = (state.avgTps === 0 && state.transactionCount === 0) ?
    ClusterStatsStatus.Ready : ClusterStatsStatus.Loading;

  if (
    action.type === PerformanceInfoActionType.SetPerfSamples &&
    action.data.samples
  ) {
    let short = action.data.samples.map((sample) => {
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
      status
    };
  }

  if (
    action.type === PerformanceInfoActionType.SetTransactionCount &&
    action.data.transactionCount
  ) {
    return {
      ...state,
      transactionCount: action.data.transactionCount,
      status
    };
  }

  if (action.type === PerformanceInfoActionType.SetErroredOut) {
    return {
      ...state,
      status: ClusterStatsStatus.Error
    };
  }

  if (
    action.type === PerformanceInfoActionType.Reset &&
    action.data.initialState
  ) {
    return {
      ...action.data.initialState,
    };
  }

  return {
    ...state
  };
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
