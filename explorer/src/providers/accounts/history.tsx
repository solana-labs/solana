import React from "react";
import {
  PublicKey,
  ConfirmedSignatureInfo,
  TransactionSignature,
  Connection,
} from "@safecoin/web3.js";
import { useCluster, Cluster } from "../cluster";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import { reportError } from "utils/sentry";

type AccountHistory = {
  fetched: ConfirmedSignatureInfo[];
  foundOldest: boolean;
};

type HistoryUpdate = {
  history?: AccountHistory;
  before?: TransactionSignature;
};

type State = Cache.State<AccountHistory>;
type Dispatch = Cache.Dispatch<HistoryUpdate>;

function combineFetched(
  fetched: ConfirmedSignatureInfo[],
  current: ConfirmedSignatureInfo[] | undefined,
  before: TransactionSignature | undefined
) {
  if (current === undefined || current.length === 0) {
    return fetched;
  }

  // History was refreshed, fetch results should be prepended if contiguous
  if (before === undefined) {
    const end = fetched.findIndex((f) => f.signature === current[0].signature);
    if (end < 0) return fetched;
    return fetched.slice(0, end).concat(current);
  }

  // More history was loaded, fetch results should be appended
  if (current[current.length - 1].signature === before) {
    return current.concat(fetched);
  }

  return fetched;
}

function reconcile(
  history: AccountHistory | undefined,
  update: HistoryUpdate | undefined
) {
  if (update?.history === undefined) return history;
  return {
    fetched: combineFetched(
      update.history.fetched,
      history?.fetched,
      update?.before
    ),
    foundOldest: update?.history?.foundOldest || history?.foundOldest || false,
  };
}

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type HistoryProviderProps = { children: React.ReactNode };
export function HistoryProvider({ children }: HistoryProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useCustomReducer(url, reconcile);

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

async function fetchAccountHistory(
  dispatch: Dispatch,
  pubkey: PublicKey,
  cluster: Cluster,
  url: string,
  options: { before?: TransactionSignature; limit: number }
) {
  dispatch({
    type: ActionType.Update,
    status: FetchStatus.Fetching,
    key: pubkey.toBase58(),
    url,
  });

  let status;
  let history;
  try {
    const connection = new Connection(url);
    const fetched = await connection.getConfirmedSignaturesForAddress2(
      pubkey,
      options
    );
    history = {
      fetched,
      foundOldest: fetched.length < options.limit,
    };
    status = FetchStatus.Fetched;
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
    status = FetchStatus.FetchFailed;
  }
  dispatch({
    type: ActionType.Update,
    url,
    key: pubkey.toBase58(),
    status,
    data: {
      history,
      before: options?.before,
    },
  });
}

export function useAccountHistories() {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useAccountHistories must be used within a AccountsProvider`
    );
  }

  return context.entries;
}

export function useAccountHistory(
  address: string
): Cache.CacheEntry<AccountHistory> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useAccountHistory must be used within a AccountsProvider`);
  }

  return context.entries[address];
}

export function useFetchAccountHistory() {
  const { cluster, url } = useCluster();
  const state = React.useContext(StateContext);
  const dispatch = React.useContext(DispatchContext);
  if (!state || !dispatch) {
    throw new Error(
      `useFetchAccountHistory must be used within a AccountsProvider`
    );
  }

  return React.useCallback(
    (pubkey: PublicKey, refresh?: boolean) => {
      const before = state.entries[pubkey.toBase58()];
      if (!refresh && before?.data?.fetched && before.data.fetched.length > 0) {
        if (before.data.foundOldest) return;
        const oldest =
          before.data.fetched[before.data.fetched.length - 1].signature;
        fetchAccountHistory(dispatch, pubkey, cluster, url, {
          before: oldest,
          limit: 25,
        });
      } else {
        fetchAccountHistory(dispatch, pubkey, cluster, url, { limit: 25 });
      }
    },
    [state, dispatch, cluster, url]
  );
}
