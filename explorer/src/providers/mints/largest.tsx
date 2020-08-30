import React from "react";
import * as Sentry from "@sentry/react";
import { useCluster, Cluster } from "providers/cluster";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import {
  PublicKey,
  Connection,
  TokenAccountBalancePair,
} from "@solana/web3.js";

type LargestAccounts = {
  largest: TokenAccountBalancePair[];
};

type State = Cache.State<LargestAccounts>;
type Dispatch = Cache.Dispatch<LargestAccounts>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type ProviderProps = { children: React.ReactNode };
export function LargestAccountsProvider({ children }: ProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useReducer<LargestAccounts>(url);

  // Clear cache whenever cluster is changed
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

async function fetchLargestAccounts(
  dispatch: Dispatch,
  pubkey: PublicKey,
  cluster: Cluster,
  url: string
) {
  dispatch({
    type: ActionType.Update,
    key: pubkey.toBase58(),
    status: Cache.FetchStatus.Fetching,
    url,
  });

  let data;
  let fetchStatus;
  try {
    data = {
      largest: (
        await new Connection(url, "single").getTokenLargestAccounts(pubkey)
      ).value,
    };
    fetchStatus = FetchStatus.Fetched;
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      Sentry.captureException(error, { tags: { url } });
    }
    fetchStatus = FetchStatus.FetchFailed;
  }
  dispatch({
    type: ActionType.Update,
    status: fetchStatus,
    data,
    key: pubkey.toBase58(),
    url,
  });
}

export function useFetchTokenLargestAccounts() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchTokenLargestAccounts must be used within a MintsProvider`
    );
  }

  const { cluster, url } = useCluster();
  return React.useCallback(
    (pubkey: PublicKey) => {
      fetchLargestAccounts(dispatch, pubkey, cluster, url);
    },
    [dispatch, cluster, url]
  );
}

export function useTokenLargestTokens(
  address: string
): Cache.CacheEntry<LargestAccounts> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useTokenLargestTokens must be used within a MintsProvider`
    );
  }

  return context.entries[address];
}
