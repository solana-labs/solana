import React from "react";
import io from "socket.io-client";

import { object, number, is, StructType, any } from "superstruct";
import { useCluster, Cluster } from "providers/cluster";

// TODO: use `partial` when it is fixed
// https://github.com/ianstormtaylor/superstruct/issues/405
const DashboardInfo = object({
  activatedStake: number(),
  avgBlockTime_1h: number(),
  avgBlockTime_1min: number(),
  circulatingSupply: number(),
  dailyPriceChange: number(),
  dailyVolume: number(),
  delinquentStake: number(),
  epochInfo: object({
    absoluteEpochStartSlot: number(),
    absoluteSlot: number(),
    blockHeight: number(),
    epoch: number(),
    slotIndex: number(),
    slotsInEpoch: number(),
  }),
  stakingYield: number(),
  tokenPrice: number(),
  totalDelegatedStake: number(),
  totalSupply: number(),
});

// TODO: use `partial` when it is fixed
// https://github.com/ianstormtaylor/superstruct/issues/405
const RootInfo = object({
  currentLeader: any(),
  nextLeaders: any(),
  root: number(),
  servedSlots: any(),
});

export const PERF_UPDATE_SEC = 5;

// TODO: use `partial` when it is fixed
// https://github.com/ianstormtaylor/superstruct/issues/405
const PerformanceInfo = object({
  avgTPS: number(),
  perfHistory: any(),
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

export type PerformanceInfo = StructType<typeof PerformanceInfo>;
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
        setPerformanceInfo(data);
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
