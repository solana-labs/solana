import React from "react";
import {
  Connection,
  TransactionSignature,
  Transaction,
  Message,
} from "@solana/web3.js";
import { useCluster, Cluster } from "../cluster";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import { reportError } from "utils/sentry";

export interface Details {
  raw?: {
    transaction: Transaction;
    message: Message;
    signatures: string[];
  } | null;
}

type State = Cache.State<Details>;
type Dispatch = Cache.Dispatch<Details>;

export const StateContext = React.createContext<State | undefined>(undefined);
export const DispatchContext = React.createContext<Dispatch | undefined>(
  undefined
);

type DetailsProviderProps = { children: React.ReactNode };
export function RawDetailsProvider({ children }: DetailsProviderProps) {
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

export function useRawTransactionDetails(
  signature: TransactionSignature
): Cache.CacheEntry<Details> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useRawTransactionDetails must be used within a TransactionsProvider`
    );
  }

  return context.entries[signature];
}

async function fetchRawTransaction(
  dispatch: Dispatch,
  signature: TransactionSignature,
  cluster: Cluster,
  url: string
) {
  let fetchStatus;
  try {
    const response = await new Connection(url).getTransaction(signature, {
      maxSupportedTransactionVersion: process.env
        .REACT_APP_MAX_SUPPORTED_TRANSACTION_VERSION
        ? parseInt(process.env.REACT_APP_MAX_SUPPORTED_TRANSACTION_VERSION, 10)
        : 0,
    });
    fetchStatus = FetchStatus.Fetched;

    let data: Details = { raw: null };
    if (response !== null) {
      const { message, signatures } = response.transaction;
      data = {
        raw: {
          message,
          signatures,
          transaction: Transaction.populate(message, signatures),
        },
      };
    }

    dispatch({
      type: ActionType.Update,
      status: fetchStatus,
      key: signature,
      data,
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
      `useFetchRawTransaction must be used within a TransactionsProvider`
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
