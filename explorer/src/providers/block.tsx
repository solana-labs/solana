import React from "react";
import * as Sentry from "@sentry/react";
import * as Cache from "providers/cache";
import { Connection, ConfirmedBlock } from "@solana/web3.js";
import { useCluster, Cluster } from "./cluster";

export enum FetchStatus {
  Fetching,
  FetchFailed,
  Fetched,
}

export enum ActionType {
  Update,
  Clear,
}

type State = Cache.State<ConfirmedBlock>;
type Dispatch = Cache.Dispatch<ConfirmedBlock>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type BlockProviderProps = { children: React.ReactNode };

export function BlockProvider({ children }: BlockProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useReducer<ConfirmedBlock>(url);

  React.useEffect(() => {
    dispatch({ type: ActionType.Clear, url });
  }, [dispatch, url]);

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        {children}
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

export function useBlock(
  key: number
): Cache.CacheEntry<ConfirmedBlock> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useBlock must be used within a BlockProvider`);
  }

  return context.entries[key];
}

export async function fetchBlock(
  dispatch: Dispatch,
  url: string,
  cluster: Cluster,
  key: number
) {
  dispatch({
    type: ActionType.Update,
    status: FetchStatus.Fetching,
    key,
    url,
  });
  let status = FetchStatus.Fetching;
  let data: ConfirmedBlock = {
    blockhash: "",
    previousBlockhash: "",
    parentSlot: 0,
    transactions: [],
  };

  try {
    const connection = new Connection(url, "max");
    data = await connection.getConfirmedBlock(Number(key));
    status = FetchStatus.Fetched;
  } catch (error) {
    console.log(error);
    if (cluster !== Cluster.Custom) {
      Sentry.captureException(error, { tags: { url } });
    }
    status = FetchStatus.FetchFailed;
  }

  dispatch({
    type: ActionType.Update,
    url,
    key,
    status,
    data,
  });
}

export function useFetchBlock() {
  const { cluster, url } = useCluster();
  const state = React.useContext(StateContext);
  const dispatch = React.useContext(DispatchContext);

  if (!state || !dispatch) {
    throw new Error(`useFetchBlock must be used within a BlockProvider`);
  }

  return React.useCallback(
    (key: number) => {
      const entry = state.entries[key];
      if (!entry) {
        fetchBlock(dispatch, url, cluster, key);
      }
    },
    [state, dispatch, cluster, url]
  );
}
