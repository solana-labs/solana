import React from "react";
import {
  Connection,
  TransactionSignature,
  ParsedConfirmedTransaction,
} from "@solana/web3.js";
import { useCluster } from "../cluster";
import { CACHED_DETAILS, isCached } from "./cached";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";

export interface Details {
  transaction?: ParsedConfirmedTransaction | null;
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
  if (isCached(url, signature)) {
    transaction = CACHED_DETAILS[signature];
    fetchStatus = FetchStatus.Fetched;
  } else {
    try {
      transaction = await new Connection(url).getParsedConfirmedTransaction(
        signature
      );
      fetchStatus = FetchStatus.Fetched;
    } catch (error) {
      console.error("Failed to fetch confirmed transaction", error);
      fetchStatus = FetchStatus.FetchFailed;
    }
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

  const { url } = useCluster();
  return (signature: TransactionSignature) => {
    url && fetchDetails(dispatch, signature, url);
  };
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
