import React from "react";

import { Supply, Connection } from "@safecoin/web3.js";
import { useCluster, ClusterStatus, Cluster } from "./cluster";
import { reportError } from "utils/sentry";

export enum Status {
  Idle,
  Disconnected,
  Connecting,
}

type State = Supply | Status | string;

type Dispatch = React.Dispatch<React.SetStateAction<State>>;
const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type Props = { children: React.ReactNode };
export function SupplyProvider({ children }: Props) {
  const [state, setState] = React.useState<State>(Status.Idle);
  const { status: clusterStatus, cluster, url } = useCluster();

  React.useEffect(() => {
    if (state !== Status.Idle) {
      if (clusterStatus === ClusterStatus.Connecting)
        setState(Status.Disconnected);
      if (clusterStatus === ClusterStatus.Connected)
        fetch(setState, cluster, url);
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
    const connection = new Connection(url, "max");
    const supply = (await connection.getSupply()).value;

    // Update state if still connecting
    dispatch((state) => {
      if (state !== Status.Connecting) return state;
      return supply;
    });
  } catch (err) {
    if (cluster !== Cluster.Custom) {
      reportError(err, { url });
    }
    dispatch("Failed to fetch supply");
  }
}

export function useSupply() {
  const state = React.useContext(StateContext);
  if (state === undefined) {
    throw new Error(`useSupply must be used within a SupplyProvider`);
  }
  return state;
}

export function useFetchSupply() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(`useFetchSupply must be used within a SupplyProvider`);
  }

  const { cluster, url } = useCluster();
  return React.useCallback(() => {
    fetch(dispatch, cluster, url);
  }, [dispatch, cluster, url]);
}
