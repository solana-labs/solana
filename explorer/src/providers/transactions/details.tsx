import React from "react";
import {
  Connection,
  TransactionSignature,
  ConfirmedTransaction,
} from "@solana/web3.js";
import { useCluster } from "../cluster";
import { useTransactions, FetchStatus } from "./index";
import { CACHED_DETAILS, isCached } from "./cached";

export interface Details {
  fetchStatus: FetchStatus;
  transaction: ConfirmedTransaction | null;
}

type State = { [signature: string]: Details };

export enum ActionType {
  Update,
  Add,
  Clear,
}

interface Update {
  type: ActionType.Update;
  signature: string;
  fetchStatus: FetchStatus;
  transaction: ConfirmedTransaction | null;
}

interface Add {
  type: ActionType.Add;
  signature: TransactionSignature;
}

interface Clear {
  type: ActionType.Clear;
}

type Action = Update | Add | Clear;
type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.Add: {
      const details = { ...state };
      const signature = action.signature;
      if (!details[signature]) {
        details[signature] = {
          fetchStatus: FetchStatus.Fetching,
          transaction: null,
        };
      }
      return details;
    }

    case ActionType.Update: {
      let details = state[action.signature];
      if (details) {
        details = {
          ...details,
          fetchStatus: action.fetchStatus,
          transaction: action.transaction,
        };
        return {
          ...state,
          [action.signature]: details,
        };
      }
      break;
    }

    case ActionType.Clear: {
      return {};
    }
  }
  return state;
}

export const StateContext = React.createContext<State | undefined>(undefined);
export const DispatchContext = React.createContext<Dispatch | undefined>(
  undefined
);

type DetailsProviderProps = { children: React.ReactNode };
export function DetailsProvider({ children }: DetailsProviderProps) {
  const [state, dispatch] = React.useReducer(reducer, {});
  const { transactions, lastFetched } = useTransactions();
  const { url } = useCluster();

  React.useEffect(() => {
    dispatch({ type: ActionType.Clear });
  }, [url]);

  // Filter blocks for current transaction slots
  React.useEffect(() => {
    if (lastFetched) {
      const confirmed =
        transactions[lastFetched] &&
        transactions[lastFetched].info?.confirmations === "max";
      const noDetails = !state[lastFetched];
      if (confirmed && noDetails) {
        dispatch({ type: ActionType.Add, signature: lastFetched });
        fetchDetails(dispatch, lastFetched, url);
      }
    }
  }, [transactions, lastFetched]); // eslint-disable-line react-hooks/exhaustive-deps

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
    fetchStatus: FetchStatus.Fetching,
    transaction: null,
    signature,
  });

  let fetchStatus;
  let transaction = null;
  if (isCached(url, signature)) {
    transaction = CACHED_DETAILS[signature];
    fetchStatus = FetchStatus.Fetched;
  } else {
    try {
      transaction = await new Connection(url).getConfirmedTransaction(
        signature
      );
      fetchStatus = FetchStatus.Fetched;
    } catch (error) {
      console.error("Failed to fetch confirmed transaction", error);
      fetchStatus = FetchStatus.FetchFailed;
    }
  }
  dispatch({ type: ActionType.Update, fetchStatus, signature, transaction });
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
