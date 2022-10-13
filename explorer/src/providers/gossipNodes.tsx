import React from "react";

import { useCluster, ClusterStatus, Cluster } from "./cluster";
import { reportError } from "utils/sentry";
import { Connection } from "@solana/web3.js";

export enum Status {
  Idle,
  Disconnected,
  Connecting,
}

export type GossipNode = {
  pubKey: string;
  ip: string | null;
  gossip: string | null;
  tpu: string | null;
  version: string | null;
};

type State = GossipNode[] | Status | string;

type Dispatch = React.Dispatch<React.SetStateAction<State>>;
const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type Props = { children: React.ReactNode };
export function GossipNodesProvider({ children }: Props) {
  const [state, setState] = React.useState<State>(Status.Idle);
  const { status: clusterStatus, cluster, url } = useCluster();

  React.useEffect(() => {
    if (state !== Status.Idle) {
      switch (clusterStatus) {
        case ClusterStatus.Connecting: {
          setState(Status.Disconnected);
          break;
        }
        case ClusterStatus.Connected: {
          fetch(setState, cluster, url);
          break;
        }
      }
    }
  }, [clusterStatus, cluster, url]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={setState}>
        {children}
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

async function fetch(dispatch: Dispatch, cluster: Cluster, url: string) {
  dispatch(Status.Connecting);

  try {
    const connection = new Connection(url, "finalized");
    let networkNodes = await connection.getClusterNodes();

    let modifiedNetworkNodes = networkNodes.map((node, i) => {
      const gossip = getIpOrPort(node.gossip, true);
      const tpu = getIpOrPort(node.tpu, true);
      const ip = getIpOrPort(node.gossip, false);

      return {
        pubKey: node.pubkey,
        ip,
        gossip,
        tpu,
        version: node.version,
      };
    });

    // Update state if still connecting
    dispatch((state) => {
      if (state !== Status.Connecting) return state;
      return modifiedNetworkNodes;
    });
  } catch (err) {
    if (cluster !== Cluster.Custom) {
      reportError(err, { url });
    }
    dispatch("Failed to fetch Gossip Nodes");
  }
}

export function useGossipNodes() {
  const state = React.useContext(StateContext);
  if (state === undefined) {
    throw new Error(`useGossipNodes must be used within a GossipNodesProvider`);
  }

  return state;
}

export function useFetchGossipNodes() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchGossipNodes must be used within a GossipNodesProvider`
    );
  }

  const { cluster, url } = useCluster();
  return React.useCallback(() => {
    fetch(dispatch, cluster, url);
  }, [dispatch, cluster, url]);
}

function getIpOrPort(str: string | null, port: boolean): string | null {
  if (!str) return null;

  if (port) {
    return str.split(":")[1];
  } else {
    return str.split(":")[0];
  }
}
