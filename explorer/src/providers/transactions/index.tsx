import React from "react";
import {
  TransactionSignature,
  Connection,
  SignatureResult,
} from "@safecoin/web3.js";
import { useCluster, Cluster } from "../cluster";
import { DetailsProvider } from "./details";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import { reportError } from "utils/sentry";
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
  cluster: Cluster,
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
  try {
    const connection = new Connection(url);
    const { value } = await connection.getSignatureStatus(signature, {
      searchTransactionHistory: true,
    });

    let info = null;
    if (value !== null) {
      let confirmations: Confirmations;
      if (typeof value.confirmations === "number") {
        confirmations = value.confirmations;
      } else {
        confirmations = "max";
      }

      let blockTime = null;
      try {
        blockTime = await connection.getBlockTime(value.slot);
      } catch (error) {
        if (cluster === Cluster.MainnetBeta && confirmations === "max") {
          reportError(error, { slot: `${value.slot}` });
        }
      }
      let timestamp: Timestamp = blockTime !== null ? blockTime : "unavailable";

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
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
    fetchStatus = FetchStatus.FetchFailed;
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
  signature: TransactionSignature | undefined
): Cache.CacheEntry<TransactionStatus> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useTransactionStatus must be used within a TransactionsProvider`
    );
  }

  if (signature === undefined) {
    return undefined;
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

  const { cluster, url } = useCluster();
  return React.useCallback(
    (signature: TransactionSignature) => {
      fetchTransactionStatus(dispatch, signature, cluster, url);
    },
    [dispatch, cluster, url]
  );
}
