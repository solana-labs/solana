import React from "react";
import io from "socket.io-client";

import { number, is, type, Infer } from "superstruct";
import { useCluster, Cluster } from "providers/cluster";

const DashboardInfo = type({
  activatedStake: number(),
  circulatingSupply: number(),
  dailyPriceChange: number(),
  dailyVolume: number(),
  delinquentStake: number(),
  stakingYield: number(),
  tokenPrice: number(),
  totalDelegatedStake: number(),
  totalSupply: number(),
});

const RootInfo = type({
  root: number(),
});

type SetActive = React.Dispatch<React.SetStateAction<boolean>>;
const SetActiveContext = React.createContext<
  { setActive: SetActive } | undefined
>(undefined);

type RootInfo = Infer<typeof RootInfo>;
type RootState = { slot: number | undefined };
const RootContext = React.createContext<RootState | undefined>(undefined);

type DashboardInfo = Infer<typeof DashboardInfo>;
type DashboardState = { info: DashboardInfo | undefined };
const DashboardContext = React.createContext<DashboardState | undefined>(
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
      setRoot(undefined);
    };
  }, [cluster]);

  return (
    <SetActiveContext.Provider value={{ setActive }}>
      <DashboardContext.Provider value={{ info: dashboardInfo }}>
        <RootContext.Provider value={{ slot: root }}>
          {children}
        </RootContext.Provider>
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

export function useSolanaBeachDashboardInfo() {
  const context = React.useContext(DashboardContext);
  if (!context) {
    throw new Error(`useDashboardInfo must be used within a StatsProvider`);
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
