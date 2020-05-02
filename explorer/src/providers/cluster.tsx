import React from "react";
import { clusterApiUrl, Connection } from "@solana/web3.js";
import { useQuery } from "../utils/url";

export enum ClusterStatus {
  Connected,
  Connecting,
  Failure
}

export enum Cluster {
  MainnetBeta,
  Testnet,
  Devnet,
  Custom
}

export const CLUSTERS = [
  Cluster.MainnetBeta,
  Cluster.Testnet,
  Cluster.Devnet,
  Cluster.Custom
];

export function clusterSlug(cluster: Cluster): string | undefined {
  switch (cluster) {
    case Cluster.MainnetBeta:
      return "mainnet-beta";
    case Cluster.Testnet:
      return "testnet";
    case Cluster.Devnet:
      return "devnet";
    case Cluster.Custom:
      return undefined;
  }
}

export function clusterName(cluster: Cluster): string {
  switch (cluster) {
    case Cluster.MainnetBeta:
      return "Mainnet Beta";
    case Cluster.Testnet:
      return "Testnet";
    case Cluster.Devnet:
      return "Devnet";
    case Cluster.Custom:
      return "Custom";
  }
}

export const MAINNET_BETA_URL = clusterApiUrl("mainnet-beta");
export const TESTNET_URL = clusterApiUrl("testnet");
export const DEVNET_URL = clusterApiUrl("devnet");

export const DEFAULT_CLUSTER = Cluster.MainnetBeta;
export const DEFAULT_CUSTOM_URL = "http://localhost:8899";

interface State {
  cluster: Cluster;
  customUrl: string;
  status: ClusterStatus;
}

interface Connecting {
  status: ClusterStatus.Connecting;
  cluster: Cluster;
  customUrl: string;
}

interface Connected {
  status: ClusterStatus.Connected;
}

interface Failure {
  status: ClusterStatus.Failure;
}

type Action = Connected | Connecting | Failure;
type Dispatch = (action: Action) => void;

function clusterReducer(state: State, action: Action): State {
  switch (action.status) {
    case ClusterStatus.Connected:
    case ClusterStatus.Failure: {
      return Object.assign({}, state, { status: action.status });
    }
    case ClusterStatus.Connecting: {
      return action;
    }
  }
}

function parseQuery(
  query: URLSearchParams
): { cluster: Cluster; customUrl: string } {
  const clusterParam = query.get("cluster");
  const clusterUrlParam = query.get("clusterUrl");

  let cluster;
  let customUrl = DEFAULT_CUSTOM_URL;
  switch (clusterUrlParam) {
    case MAINNET_BETA_URL:
      cluster = Cluster.MainnetBeta;
      break;
    case DEVNET_URL:
      cluster = Cluster.Devnet;
      break;
    case TESTNET_URL:
      cluster = Cluster.Testnet;
      break;
  }

  switch (clusterParam) {
    case "mainnet-beta":
      cluster = Cluster.MainnetBeta;
      break;
    case "devnet":
      cluster = Cluster.Devnet;
      break;
    case "testnet":
      cluster = Cluster.Testnet;
      break;
  }

  if (!cluster) {
    if (!clusterUrlParam) {
      cluster = DEFAULT_CLUSTER;
    } else {
      cluster = Cluster.Custom;
      customUrl = clusterUrlParam;
    }
  }

  return {
    cluster,
    customUrl
  };
}

type SetShowModal = React.Dispatch<React.SetStateAction<boolean>>;
const ModalContext = React.createContext<[boolean, SetShowModal] | undefined>(
  undefined
);
const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type ClusterProviderProps = { children: React.ReactNode };
export function ClusterProvider({ children }: ClusterProviderProps) {
  const [state, dispatch] = React.useReducer(clusterReducer, {
    cluster: DEFAULT_CLUSTER,
    customUrl: DEFAULT_CUSTOM_URL,
    status: ClusterStatus.Connecting
  });
  const [showModal, setShowModal] = React.useState(false);
  const { cluster, customUrl } = parseQuery(useQuery());

  // Reconnect to cluster when it changes
  React.useEffect(() => {
    updateCluster(dispatch, cluster, customUrl);
  }, [cluster, customUrl]);

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        <ModalContext.Provider value={[showModal, setShowModal]}>
          {children}
        </ModalContext.Provider>
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

export function clusterUrl(cluster: Cluster, customUrl: string): string {
  switch (cluster) {
    case Cluster.Devnet:
      return DEVNET_URL;
    case Cluster.MainnetBeta:
      return MAINNET_BETA_URL;
    case Cluster.Testnet:
      return TESTNET_URL;
    case Cluster.Custom:
      return customUrl;
  }
}

async function updateCluster(
  dispatch: Dispatch,
  cluster: Cluster,
  customUrl: string
) {
  dispatch({
    status: ClusterStatus.Connecting,
    cluster,
    customUrl
  });

  try {
    const connection = new Connection(clusterUrl(cluster, customUrl));
    await connection.getRecentBlockhash();
    dispatch({ status: ClusterStatus.Connected });
  } catch (error) {
    console.error("Failed to update cluster", error);
    dispatch({ status: ClusterStatus.Failure });
  }
}

export function useCluster() {
  const context = React.useContext(StateContext);
  if (!context) {
    throw new Error(`useCluster must be used within a ClusterProvider`);
  }
  return {
    ...context,
    url: clusterUrl(context.cluster, context.customUrl),
    name: clusterName(context.cluster)
  };
}

export function useClusterModal() {
  const context = React.useContext(ModalContext);
  if (!context) {
    throw new Error(`useClusterModal must be used within a ClusterProvider`);
  }
  return context;
}
