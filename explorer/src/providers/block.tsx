import React from "react";
import * as Sentry from "@sentry/react";
import * as Cache from "providers/cache";
import { Connection, ConfirmedBlock } from "@safecoin/web3.js";
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

type Block = {
  block?: ConfirmedBlock;
};

type State = Cache.State<Block>;
type Dispatch = Cache.Dispatch<Block>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type BlockProviderProps = { children: React.ReactNode };

export function BlockProvider({ children }: BlockProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useReducer<Block>(url);

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

export function useBlock(key: number): Cache.CacheEntry<Block> | undefined {
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

  let status: FetchStatus;
  let data: Block | undefined = undefined;

  try {
    const connection = new Connection(url, "max");
    data = { block: await connection.getConfirmedBlock(Number(key)) };
    status = FetchStatus.Fetched;
  } catch (err) {
    const error = err as Error;
    if (error.message.includes("not found")) {
      data = {} as Block;
      status = FetchStatus.Fetched;
    } else {
      status = FetchStatus.FetchFailed;
      if (cluster !== Cluster.Custom) {
        Sentry.captureException(error, { tags: { url } });
      }
    }
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
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(`useFetchBlock must be used within a BlockProvider`);
  }

  const { cluster, url } = useCluster();
  return React.useCallback(
    (key: number) => fetchBlock(dispatch, url, cluster, key),
    [dispatch, cluster, url]
  );
}
