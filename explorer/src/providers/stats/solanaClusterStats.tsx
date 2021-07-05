import React from "react";
import { Connection } from "@solana/web3.js";
import { useCluster, Cluster } from "providers/cluster";
import {
  DashboardInfo,
  DashboardInfoActionType,
  dashboardInfoReducer,
} from "./solanaDashboardInfo";
import {
  PerformanceInfo,
  PerformanceInfoActionType,
  performanceInfoReducer,
} from "./solanaPerformanceInfo";
import { reportError } from "utils/sentry";

export const PERF_UPDATE_SEC = 5;
export const SAMPLE_HISTORY_HOURS = 6;
export const PERFORMANCE_SAMPLE_INTERVAL = 60000;
export const TRANSACTION_COUNT_INTERVAL = 5000;
export const EPOCH_INFO_INTERVAL = 2000;
export const BLOCK_TIME_INTERVAL = 5000;
export const LOADING_TIMEOUT = 10000;

export enum ClusterStatsStatus {
  Loading,
  Ready,
  Error,
}

const initialPerformanceInfo: PerformanceInfo = {
  status: ClusterStatsStatus.Loading,
  avgTps: 0,
  historyMaxTps: 0,
  perfHistory: {
    short: [],
    medium: [],
    long: [],
  },
  transactionCount: 0,
};

const initialDashboardInfo: DashboardInfo = {
  status: ClusterStatsStatus.Loading,
  avgSlotTime_1h: 0,
  avgSlotTime_1min: 0,
  epochInfo: {
    absoluteSlot: 0,
    blockHeight: 0,
    epoch: 0,
    slotIndex: 0,
    slotsInEpoch: 0,
  },
};

type SetActive = React.Dispatch<React.SetStateAction<boolean>>;
const StatsProviderContext = React.createContext<
  | {
      setActive: SetActive;
      setTimedOut: Function;
      retry: Function;
      active: boolean;
    }
  | undefined
>(undefined);

type DashboardState = { info: DashboardInfo };
const DashboardContext = React.createContext<DashboardState | undefined>(
  undefined
);

type PerformanceState = { info: PerformanceInfo };
const PerformanceContext = React.createContext<PerformanceState | undefined>(
  undefined
);

type Props = { children: React.ReactNode };
export function SafecoinClusterStatsProvider({ children }: Props) {
  const { cluster, url } = useCluster();
  const [active, setActive] = React.useState(false);
  const [dashboardInfo, dispatchDashboardInfo] = React.useReducer(
    dashboardInfoReducer,
    initialDashboardInfo
  );
  const [performanceInfo, dispatchPerformanceInfo] = React.useReducer(
    performanceInfoReducer,
    initialPerformanceInfo
  );

  React.useEffect(() => {
    if (!active || !url) return;

    const connection = new Connection(url);
    let lastSlot: number | null = null;

    const getPerformanceSamples = async () => {
      try {
        const samples = await connection.getRecentPerformanceSamples(
          60 * SAMPLE_HISTORY_HOURS
        );

        if (samples.length < 1) {
          // no samples to work with (node has no history).
          return; // we will allow for a timeout instead of throwing an error
        }

        dispatchPerformanceInfo({
          type: PerformanceInfoActionType.SetPerfSamples,
          data: samples,
        });

        dispatchDashboardInfo({
          type: DashboardInfoActionType.SetPerfSamples,
          data: samples,
        });
      } catch (error) {
        if (cluster !== Cluster.Custom) {
          reportError(error, { url });
        }
        dispatchPerformanceInfo({
          type: PerformanceInfoActionType.SetError,
          data: error.toString(),
        });
        dispatchDashboardInfo({
          type: DashboardInfoActionType.SetError,
          data: error.toString(),
        });
        setActive(false);
      }
    };

    const getTransactionCount = async () => {
      try {
        const transactionCount = await connection.getTransactionCount();
        dispatchPerformanceInfo({
          type: PerformanceInfoActionType.SetTransactionCount,
          data: transactionCount,
        });
      } catch (error) {
        if (cluster !== Cluster.Custom) {
          reportError(error, { url });
        }
        dispatchPerformanceInfo({
          type: PerformanceInfoActionType.SetError,
          data: error.toString(),
        });
        setActive(false);
      }
    };

    const getEpochInfo = async () => {
      try {
        const epochInfo = await connection.getEpochInfo();
        lastSlot = epochInfo.absoluteSlot;
        dispatchDashboardInfo({
          type: DashboardInfoActionType.SetEpochInfo,
          data: epochInfo,
        });
      } catch (error) {
        if (cluster !== Cluster.Custom) {
          reportError(error, { url });
        }
        dispatchDashboardInfo({
          type: DashboardInfoActionType.SetError,
          data: error.toString(),
        });
        setActive(false);
      }
    };

    const getBlockTime = async () => {
      if (lastSlot) {
        try {
          const blockTime = await connection.getBlockTime(lastSlot);
          if (blockTime !== null) {
            dispatchDashboardInfo({
              type: DashboardInfoActionType.SetLastBlockTime,
              data: {
                slot: lastSlot,
                blockTime: blockTime * 1000,
              },
            });
          }
        } catch (error) {
          // let this fail gracefully
        }
      }
    };

    const performanceInterval = setInterval(
      getPerformanceSamples,
      PERFORMANCE_SAMPLE_INTERVAL
    );
    const transactionCountInterval = setInterval(
      getTransactionCount,
      TRANSACTION_COUNT_INTERVAL
    );
    const epochInfoInterval = setInterval(getEpochInfo, EPOCH_INFO_INTERVAL);
    const blockTimeInterval = setInterval(getBlockTime, BLOCK_TIME_INTERVAL);

    getPerformanceSamples();
    getTransactionCount();
    (async () => {
      await getEpochInfo();
      await getBlockTime();
    })();

    return () => {
      clearInterval(performanceInterval);
      clearInterval(transactionCountInterval);
      clearInterval(epochInfoInterval);
      clearInterval(blockTimeInterval);
    };
  }, [active, cluster, url]);

  // Reset when cluster changes
  React.useEffect(() => {
    return () => {
      resetData();
    };
  }, [url]);

  function resetData() {
    dispatchDashboardInfo({
      type: DashboardInfoActionType.Reset,
      data: initialDashboardInfo,
    });
    dispatchPerformanceInfo({
      type: PerformanceInfoActionType.Reset,
      data: initialPerformanceInfo,
    });
  }

  const setTimedOut = React.useCallback(() => {
    dispatchDashboardInfo({
      type: DashboardInfoActionType.SetError,
      data: "Cluster stats timed out",
    });
    dispatchPerformanceInfo({
      type: PerformanceInfoActionType.SetError,
      data: "Cluster stats timed out",
    });
    console.error("Cluster stats timed out");
    setActive(false);
  }, []);

  const retry = React.useCallback(() => {
    resetData();
    setActive(true);
  }, []);

  return (
    <StatsProviderContext.Provider
      value={{ setActive, setTimedOut, retry, active }}
    >
      <DashboardContext.Provider value={{ info: dashboardInfo }}>
        <PerformanceContext.Provider value={{ info: performanceInfo }}>
          {children}
        </PerformanceContext.Provider>
      </DashboardContext.Provider>
    </StatsProviderContext.Provider>
  );
}

export function useStatsProvider() {
  const context = React.useContext(StatsProviderContext);
  if (!context) {
    throw new Error(`useContext must be used within a StatsProvider`);
  }
  return context;
}

export function useDashboardInfo() {
  const context = React.useContext(DashboardContext);
  if (!context) {
    throw new Error(`useDashboardInfo must be used within a StatsProvider`);
  }
  return context.info;
}

export function usePerformanceInfo() {
  const context = React.useContext(PerformanceContext);
  if (!context) {
    throw new Error(`usePerformanceInfo must be used within a StatsProvider`);
  }
  return context.info;
}
