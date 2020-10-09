import React from "react";
import * as Sentry from "@sentry/react";
import * as Cache from "providers/cache";
import { RetrieveSlot, RetrieveBlockhash } from "@theronin/solarweave";
import { useCluster, Cluster } from "../cluster";
import { ActionType, FetchStatus } from "providers/cache";
import { HistoryProvider } from "./history";
import { TokensProvider } from "./tokens";

type State = Cache.State<Block>;
type Dispatch = Cache.Dispatch<Block>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type BlockProviderProps = { children: React.ReactNode };

export interface Block {
  BlockData: any;
  Tags: any;
}

export function BlockProvider({ children }: BlockProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useReducer<Block>(url);

  React.useEffect(() => {
    dispatch({ type: ActionType.Clear, url });
  }, [dispatch, url]);

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        <TokensProvider>
          <HistoryProvider>{children}</HistoryProvider>
        </TokensProvider>
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

export function useBlock(key: string): Cache.CacheEntry<Block> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useBlock must be used within a BlockProvider`);
  }

  return context.entries[key];
}

export async function fetchBlock(
  dispatch: Dispatch,
  url: string,
  solarweave: string,
  cluster: Cluster,
  key: string,
  type: string
) {
  dispatch({
    type: ActionType.Update,
    status: FetchStatus.Fetching,
    key,
    url,
  });

  let result;
  let status = FetchStatus.Fetching;

  try {
    if (type === "slot") {
      result = await RetrieveSlot(key, solarweave);
    } else {
      result = await RetrieveBlockhash(key, solarweave);
    }

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
    data: {
      BlockData: result?.BlockData,
      Tags: result?.Tags,
    },
  });
}

export function useFetchBlock() {
  const { cluster, url, solarweave } = useCluster();
  const state = React.useContext(StateContext);
  const dispatch = React.useContext(DispatchContext);

  if (!state || !dispatch) {
    throw new Error(`useBlock must be used within a BlockProvider`);
  }

  return React.useCallback(
    (key: string, type: string) => {
      const entry = state.entries[key];
      if (!entry) {
        fetchBlock(dispatch, url, solarweave, cluster, key, type);
      }
    },
    [state, dispatch, cluster, url, solarweave]
  );
}
