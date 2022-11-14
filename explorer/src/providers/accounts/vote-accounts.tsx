import { Connection, VoteAccountStatus } from "@solana/web3.js";
import { useCluster, ClusterStatus, Cluster } from "../cluster";
import React from "react";
import { reportError } from "utils/sentry";

export enum Status {
  Idle,
  Disconnected,
  Connecting,
}

type State = VoteAccountStatus | Status | string;
type Dispatch = React.Dispatch<React.SetStateAction<State>>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type Props = { children: React.ReactNode };

export function VoteAccountsProvider({ children }: Props) {
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
          fetchVoteAccounts(setState, cluster, url);
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

async function fetchVoteAccounts(
  dispatch: Dispatch,
  cluster: Cluster,
  url: string
) {
  dispatch(Status.Connecting);

  try {
    const connection = new Connection(url);
    const result = await connection.getVoteAccounts();

    dispatch((state) => {
      if (state !== Status.Connecting) return state;
      return result;
    });
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
    dispatch("Failed to fetch Vote Accounts");
  }
}

export function useFetchVoteAccounts() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchVoteAccounts must be used within a GossipNodesProvider`
    );
  }

  const { cluster, url } = useCluster();
  return React.useCallback(() => {
    fetchVoteAccounts(dispatch, cluster, url);
  }, [dispatch, cluster, url]);
}

export function useVoteAccounts() {
  const state = React.useContext(StateContext);
  if (state === undefined) {
    throw new Error(`useVoteAccounts must be used within a GossipNodesProvider`);
  }

  return state;
}