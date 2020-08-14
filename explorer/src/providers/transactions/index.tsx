import React from "react";
import * as Sentry from "@sentry/react";
import {
  TransactionSignature,
  Connection,
  SignatureResult,
} from "@solana/web3.js";
import { useCluster } from "../cluster";
import { DetailsProvider } from "./details";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import { CACHED_STATUSES, isCached } from "./cached";
export { useTransactionDetails } from "./details";

export type Confirmations = number | "max";

export type Timestamp = number | "unavailable";

export interface TransactionStatusInfo {
  slot: number;
  result: SignatureResult;
  timestamp: Timestamp;
  confirmations: Confirmations;
}

export interface TransactionStatus {
  signature: TransactionSignature;
  info: TransactionStatusInfo | null;
}

export const TX_ALIASES = ["tx", "txn", "transaction"];

type State = Cache.State<TransactionStatus>;
type Dispatch = Cache.Dispatch<TransactionStatus>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type TransactionsProviderProps = { children: React.ReactNode };
export function TransactionsProvider({ children }: TransactionsProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useReducer<TransactionStatus>(url);

  // Clear accounts cache whenever cluster is changed
  React.useEffect(() => {
    dispatch({ type: ActionType.Clear, url });
  }, [dispatch, url]);

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        <DetailsProvider>{children}</DetailsProvider>
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

export async function fetchTransactionStatus(
  dispatch: Dispatch,
  signature: TransactionSignature,
  url: string
) {
  dispatch({
    type: ActionType.Update,
    key: signature,
    status: FetchStatus.Fetching,
    url,
  });

  let fetchStatus;
  let data;
  if (isCached(url, signature)) {
    const info = CACHED_STATUSES[signature];
    data = { signature, info };
    fetchStatus = FetchStatus.Fetched;
  } else {
    try {
      const connection = new Connection(url);
      const { value } = await connection.getSignatureStatus(signature, {
        searchTransactionHistory: true,
      });

      let info = null;
      if (value !== null) {
        let blockTime = null;
        try {
          blockTime = await connection.getBlockTime(value.slot);
        } catch (error) {
          Sentry.captureException(error, { tags: { slot: `${value.slot}` } });
        }
        let timestamp: Timestamp =
          blockTime !== null ? blockTime : "unavailable";

        let confirmations: Confirmations;
        if (typeof value.confirmations === "number") {
          confirmations = value.confirmations;
        } else {
          confirmations = "max";
        }

        info = {
          slot: value.slot,
          timestamp,
          confirmations,
          result: { err: value.err },
        };
      }
      data = { signature, info };
      fetchStatus = FetchStatus.Fetched;
    } catch (error) {
      Sentry.captureException(error, { tags: { url } });
      fetchStatus = FetchStatus.FetchFailed;
    }
  }

  dispatch({
    type: ActionType.Update,
    key: signature,
    status: fetchStatus,
    data,
    url,
  });
}

export function useTransactions() {
  const context = React.useContext(StateContext);
  if (!context) {
    throw new Error(
      `useTransactions must be used within a TransactionsProvider`
    );
  }
  return context;
}

export function useTransactionStatus(
  signature: TransactionSignature
): Cache.CacheEntry<TransactionStatus> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useTransactionStatus must be used within a TransactionsProvider`
    );
  }

  return context.entries[signature];
}

export function useFetchTransactionStatus() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchTransactionStatus must be used within a TransactionsProvider`
    );
  }

  const { url } = useCluster();
  return (signature: TransactionSignature) => {
    fetchTransactionStatus(dispatch, signature, url);
  };
}
