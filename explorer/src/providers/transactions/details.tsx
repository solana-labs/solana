import React from "react";
import {
  Connection,
  TransactionSignature,
  ParsedConfirmedTransaction,
  ConfirmedTransaction,
} from "@safecoin/web3.js";
import { useCluster, Cluster } from "../cluster";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import { reportError } from "utils/sentry";

export interface Details {
  transaction?: ParsedConfirmedTransaction | null;
  raw?: ConfirmedTransaction | null;
}

type State = Cache.State<Details>;
type Dispatch = Cache.Dispatch<Details>;

export const StateContext = React.createContext<State | undefined>(undefined);
export const DispatchContext = React.createContext<Dispatch | undefined>(
  undefined
);

type DetailsProviderProps = { children: React.ReactNode };
export function DetailsProvider({ children }: DetailsProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useReducer<Details>(url);

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

async function fetchDetails(
  dispatch: Dispatch,
  signature: TransactionSignature,
  cluster: Cluster,
  url: string
) {
  dispatch({
    type: ActionType.Update,
    status: FetchStatus.Fetching,
    key: signature,
    url,
  });

  let fetchStatus;
  let transaction;
  try {
    transaction = await new Connection(url).getParsedConfirmedTransaction(
      signature
    );
    fetchStatus = FetchStatus.Fetched;
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
    fetchStatus = FetchStatus.FetchFailed;
  }
  dispatch({
    type: ActionType.Update,
    status: fetchStatus,
    key: signature,
    data: { transaction },
    url,
  });
}

export function useFetchTransactionDetails() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchTransactionDetails must be used within a TransactionsProvider`
    );
  }

  const { cluster, url } = useCluster();
  return React.useCallback(
    (signature: TransactionSignature) => {
      url && fetchDetails(dispatch, signature, cluster, url);
    },
    [dispatch, cluster, url]
  );
}

export function useTransactionDetails(
  signature: TransactionSignature
): Cache.CacheEntry<Details> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useTransactionDetails must be used within a TransactionsProvider`
    );
  }

  return context.entries[signature];
}

export type TransactionDetailsCache = {
  [key: string]: Cache.CacheEntry<Details>;
};
export function useTransactionDetailsCache(): TransactionDetailsCache {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useTransactionDetailsCache must be used within a TransactionsProvider`
    );
  }

  return context.entries;
}

async function fetchRawTransaction(
  dispatch: Dispatch,
  signature: TransactionSignature,
  cluster: Cluster,
  url: string
) {
  let fetchStatus;
  let transaction;
  try {
    transaction = await new Connection(url).getConfirmedTransaction(signature);
    fetchStatus = FetchStatus.Fetched;
    dispatch({
      type: ActionType.Update,
      status: fetchStatus,
      key: signature,
      data: {
        raw: transaction,
      },
      url,
    });
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
  }
}

export function useFetchRawTransaction() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchRawTransaaction must be used within a TransactionsProvider`
    );
  }

  const { cluster, url } = useCluster();
  return React.useCallback(
    (signature: TransactionSignature) => {
      url && fetchRawTransaction(dispatch, signature, cluster, url);
    },
    [dispatch, cluster, url]
  );
}
