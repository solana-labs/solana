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

const initialPerformanceInfo: PerformanceInfo = {
  loading: true,
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
  loading: true,
  avgBlockTime_1h: 0,
  avgBlockTime_1min: 0,
  epochInfo: {
    absoluteSlot: 0,
    blockHeight: 0,
    epoch: 0,
    slotIndex: 0,
    slotsInEpoch: 0,
  },
};

type SetActive = React.Dispatch<React.SetStateAction<boolean>>;
const SetActiveContext = React.createContext<
  { setActive: SetActive } | undefined
>(undefined);

type DashboardState = { info: DashboardInfo | undefined };
const DashboardContext = React.createContext<DashboardState | undefined>(
  undefined
);

type PerformanceState = { info: PerformanceInfo | undefined };
const PerformanceContext = React.createContext<PerformanceState | undefined>(
  undefined
);

type Props = { children: React.ReactNode };
export function SolanaClusterStatsProvider({ children }: Props) {
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

    const getPerformanceSamples = async () => {
      try {
        const samples = await connection.getRecentPerformanceSamples(
          60 * SAMPLE_HISTORY_HOURS
        );

        dispatchPerformanceInfo({
          type: PerformanceInfoActionType.SetPerfSamples,
          data: {
            samples: samples,
          },
        });

        dispatchDashboardInfo({
          type: DashboardInfoActionType.SetPerfSamples,
          data: {
            samples: samples,
          },
        });
      } catch (error) {
        if (cluster !== Cluster.Custom) {
          reportError(error, { url });
        }
      }
    };

    const getTransactionCount = async () => {
      try {
        const transactionCount = await connection.getTransactionCount();
        dispatchPerformanceInfo({
          type: PerformanceInfoActionType.SetTransactionCount,
          data: {
            transactionCount: transactionCount,
          },
        });
      } catch (error) {
        if (cluster !== Cluster.Custom) {
          reportError(error, { url });
        }
      }
    };

    const getEpochInfo = async () => {
      try {
        const epochInfo = await connection.getEpochInfo();
        dispatchDashboardInfo({
          type: DashboardInfoActionType.SetEpochInfo,
          data: {
            epochInfo: epochInfo,
          },
        });
      } catch (error) {
        if (cluster !== Cluster.Custom) {
          reportError(error, { url });
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

    getPerformanceSamples();
    getTransactionCount();
    getEpochInfo();

    return () => {
      clearInterval(performanceInterval);
      clearInterval(transactionCountInterval);
      clearInterval(epochInfoInterval);
    };
  }, [active, cluster, url]);

  // Reset when cluster changes
  React.useEffect(() => {
    return () => {
      dispatchDashboardInfo({
        type: DashboardInfoActionType.Reset,
        data: {
          initialState: initialDashboardInfo,
        },
      });
      dispatchPerformanceInfo({
        type: PerformanceInfoActionType.Reset,
        data: {
          initialState: initialPerformanceInfo,
        },
      });
    };
  }, [cluster]);

  return (
    <SetActiveContext.Provider value={{ setActive }}>
      <DashboardContext.Provider value={{ info: dashboardInfo }}>
        <PerformanceContext.Provider value={{ info: performanceInfo }}>
          {children}
        </PerformanceContext.Provider>
      </DashboardContext.Provider>
    </SetActiveContext.Provider>
  );
}

export function useSetActive() {
  const context = React.useContext(SetActiveContext);
  if (!context) {
    throw new Error(`useSetActive must be used within a StatsProvider`);
  }
  return context.setActive;
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
