import React from "react";
import { clusterApiUrl, Connection } from "@safecoin/web3.js";
import { useQuery } from "../utils/url";
import { useHistory, useLocation } from "react-router-dom";
import { reportError } from "utils/sentry";

export enum ClusterStatus {
  Connected,
  Connecting,
  Failure,
}

export enum Cluster {
  MainnetBeta,
  Testnet,
  Devnet,
  Custom,
}

export const CLUSTERS = [
  Cluster.MainnetBeta,
  Cluster.Testnet,
  Cluster.Devnet,
  Cluster.Custom,
];

export function clusterSlug(cluster: Cluster): string {
  switch (cluster) {
    case Cluster.MainnetBeta:
      return "mainnet-beta";
    case Cluster.Testnet:
      return "testnet";
    case Cluster.Devnet:
      return "devnet";
    case Cluster.Custom:
      return "custom";
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

export const DEFAULT_CLUSTER = Cluster.MainnetBeta;

interface State {
  cluster: Cluster;
  customUrl: string;
  firstAvailableBlock?: number;
  status: ClusterStatus;
}

interface Action {
  status: ClusterStatus;
  cluster: Cluster;
  customUrl: string;
  firstAvailableBlock?: number;
}

type Dispatch = (action: Action) => void;

function clusterReducer(state: State, action: Action): State {
  switch (action.status) {
    case ClusterStatus.Connected:
    case ClusterStatus.Failure: {
      if (
        state.cluster !== action.cluster ||
        state.customUrl !== action.customUrl
      )
        return state;
      return action;
    }
    case ClusterStatus.Connecting: {
      return action;
    }
  }
}

function parseQuery(query: URLSearchParams): Cluster {
  const clusterParam = query.get("cluster");
  switch (clusterParam) {
    case "custom":
      return Cluster.Custom;
    case "devnet":
      return Cluster.Devnet;
    case "testnet":
      return Cluster.Testnet;
    case "mainnet-beta":
    default:
      return Cluster.MainnetBeta;
  }
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
    customUrl: "",
    status: ClusterStatus.Connecting,
  });
  const [showModal, setShowModal] = React.useState(false);
  const query = useQuery();
  const cluster = parseQuery(query);
  const enableCustomUrl = localStorage.getItem("enableCustomUrl") !== null;
  const customUrl = enableCustomUrl
    ? query.get("customUrl") || ""
    : state.customUrl;
  const history = useHistory();
  const location = useLocation();

  // Remove customUrl param if dev setting is disabled
  React.useEffect(() => {
    if (!enableCustomUrl && query.has("customUrl")) {
      query.delete("customUrl");
      history.push({ ...location, search: query.toString() });
    }
  }, [enableCustomUrl]); // eslint-disable-line react-hooks/exhaustive-deps

  // Reconnect to cluster when params change
  React.useEffect(() => {
    if (cluster === Cluster.Custom) {
      // Remove cluster param if custom url has not been set
      if (customUrl.length === 0) {
        query.delete("cluster");
        history.push({ ...location, search: query.toString() });
        return;
      }
    }

    updateCluster(dispatch, cluster, customUrl);
  }, [cluster, customUrl]); // eslint-disable-line react-hooks/exhaustive-deps

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

async function updateCluster(
  dispatch: Dispatch,
  cluster: Cluster,
  customUrl: string
) {
  dispatch({
    status: ClusterStatus.Connecting,
    cluster,
    customUrl,
  });

  try {
    const connection = new Connection(clusterUrl(cluster, customUrl));
    const firstAvailableBlock = await connection.getFirstAvailableBlock();
    dispatch({
      status: ClusterStatus.Connected,
      cluster,
      customUrl,
      firstAvailableBlock,
    });
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { clusterUrl: clusterUrl(cluster, customUrl) });
    }
    dispatch({
      status: ClusterStatus.Failure,
      cluster,
      customUrl,
    });
  }
}

export function useUpdateCustomUrl() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(`useUpdateCustomUrl must be used within a ClusterProvider`);
  }

  return (customUrl: string) => {
    updateCluster(dispatch, Cluster.Custom, customUrl);
  };
}

export function useCluster() {
  const context = React.useContext(StateContext);
  if (!context) {
    throw new Error(`useCluster must be used within a ClusterProvider`);
  }
  return {
    ...context,
    url: clusterUrl(context.cluster, context.customUrl),
    name: clusterName(context.cluster),
  };
}

export function useClusterModal() {
  const context = React.useContext(ModalContext);
  if (!context) {
    throw new Error(`useClusterModal must be used within a ClusterProvider`);
  }
  return context;
}
