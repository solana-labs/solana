import React from "react";
import io from "socket.io-client";

import { pick, array, nullable, number, is, StructType } from "superstruct";
import { useCluster, Cluster } from "providers/cluster";

const DashboardInfo = pick({
  avgBlockTime_1h: number(),
  avgBlockTime_1min: number(),
  epochInfo: pick({
    absoluteSlot: number(),
    blockHeight: number(),
    epoch: number(),
    slotIndex: number(),
    slotsInEpoch: number(),
  }),
});

const RootInfo = pick({
  root: number(),
});

export const PERF_UPDATE_SEC = 5;

const PerformanceInfo = pick({
  avgTPS: number(),
  perfHistory: pick({
    s: array(nullable(number())),
    m: array(nullable(number())),
    l: array(nullable(number())),
  }),
  totalTransactionCount: number(),
});

type SetActive = React.Dispatch<React.SetStateAction<boolean>>;
const SetActiveContext = React.createContext<
  { setActive: SetActive } | undefined
>(undefined);

type RootInfo = StructType<typeof RootInfo>;
type RootState = { slot: number | undefined };
const RootContext = React.createContext<RootState | undefined>(undefined);

type DashboardInfo = StructType<typeof DashboardInfo>;
type DashboardState = { info: DashboardInfo | undefined };
const DashboardContext = React.createContext<DashboardState | undefined>(
  undefined
);

export type PerformanceInfo = {
  avgTps: number;
  historyMaxTps: number;
  perfHistory: {
    short: (number | null)[];
    medium: (number | null)[];
    long: (number | null)[];
  };
  transactionCount: number;
};

type PerformanceState = { info: PerformanceInfo | undefined };
const PerformanceContext = React.createContext<PerformanceState | undefined>(
  undefined
);

const MAINNET_URL = "https://api.solanabeach.io:8443/mainnet";
const TESTNET_URL = "https://api.solanabeach.io:8443/tds";

type Props = { children: React.ReactNode };
export function SolanaBeachProvider({ children }: Props) {
  const { cluster } = useCluster();
  const [active, setActive] = React.useState(false);
  const [root, setRoot] = React.useState<number>();
  const [dashboardInfo, setDashboardInfo] = React.useState<DashboardInfo>();
  const [performanceInfo, setPerformanceInfo] = React.useState<
    PerformanceInfo
  >();

  React.useEffect(() => {
    if (!active) return;

    let socket: SocketIOClient.Socket;
    if (cluster === Cluster.MainnetBeta) {
      socket = io(MAINNET_URL);
    } else if (cluster === Cluster.Testnet) {
      socket = io(TESTNET_URL);
    } else {
      return;
    }

    socket.on("connect", () => {
      socket.emit("request_dashboardInfo");
      socket.emit("request_performanceInfo");
    });
    socket.on("error", (err: any) => {
      console.error(err);
    });
    socket.on("dashboardInfo", (data: any) => {
      if (is(data, DashboardInfo)) {
        setDashboardInfo(data);
      }
    });
    socket.on("performanceInfo", (data: any) => {
      if (is(data, PerformanceInfo)) {
        const trimSeries = (series: (number | null)[]) => {
          return series.slice(series.length - 51, series.length - 1);
        };

        const seriesMax = (series: (number | null)[]) => {
          return series.reduce((max: number, next) => {
            if (next === null) return max;
            return Math.max(max, next);
          }, 0);
        };

        const normalize = (series: Array<number | null>, seconds: number) => {
          return series.map((next) => {
            if (next === null) return next;
            return Math.round(next / seconds);
          });
        };

        const short = normalize(trimSeries(data.perfHistory.s), 15);
        const medium = normalize(trimSeries(data.perfHistory.m), 60);
        const long = normalize(trimSeries(data.perfHistory.l), 180);
        const historyMaxTps = Math.max(
          seriesMax(short),
          seriesMax(medium),
          seriesMax(long)
        );

        setPerformanceInfo({
          avgTps: data.avgTPS,
          historyMaxTps,
          perfHistory: { short, medium, long },
          transactionCount: data.totalTransactionCount,
        });
      }
    });
    socket.on("rootNotification", (data: any) => {
      if (is(data, RootInfo)) {
        setRoot(data.root);
      }
    });
    return () => {
      socket.disconnect();
    };
  }, [active, cluster]);

  // Reset info whenever the cluster changes
  React.useEffect(() => {
    return () => {
      setDashboardInfo(undefined);
      setPerformanceInfo(undefined);
      setRoot(undefined);
    };
  }, [cluster]);

  return (
    <SetActiveContext.Provider value={{ setActive }}>
      <DashboardContext.Provider value={{ info: dashboardInfo }}>
        <PerformanceContext.Provider value={{ info: performanceInfo }}>
          <RootContext.Provider value={{ slot: root }}>
            {children}
          </RootContext.Provider>
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

export function useRootSlot() {
  const context = React.useContext(RootContext);
  if (!context) {
    throw new Error(`useRootSlot must be used within a StatsProvider`);
  }
  return context.slot;
}
