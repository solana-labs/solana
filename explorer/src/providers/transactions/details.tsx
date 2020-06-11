import React from "react";
import {
  Connection,
  TransactionSignature,
  ConfirmedTransaction
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
  Remove
}

interface Update {
  type: ActionType.Update;
  signature: string;
  fetchStatus: FetchStatus;
  transaction: ConfirmedTransaction | null;
}

interface Add {
  type: ActionType.Add;
  signatures: TransactionSignature[];
}

interface Remove {
  type: ActionType.Remove;
  signatures: TransactionSignature[];
}

type Action = Update | Add | Remove;
type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.Add: {
      if (action.signatures.length === 0) return state;
      const details = { ...state };
      action.signatures.forEach(signature => {
        if (!details[signature]) {
          details[signature] = {
            fetchStatus: FetchStatus.Fetching,
            transaction: null
          };
        }
      });
      return details;
    }

    case ActionType.Remove: {
      if (action.signatures.length === 0) return state;
      const details = { ...state };
      action.signatures.forEach(signature => {
        delete details[signature];
      });
      return details;
    }

    case ActionType.Update: {
      let details = state[action.signature];
      if (details) {
        details = {
          ...details,
          fetchStatus: action.fetchStatus,
          transaction: action.transaction
        };
        return {
          ...state,
          [action.signature]: details
        };
      }
      break;
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

  const { transactions } = useTransactions();
  const { url } = useCluster();

  // Filter blocks for current transaction slots
  React.useEffect(() => {
    const removeSignatures = new Set<string>();
    const fetchSignatures = new Set<string>();
    transactions.forEach(({ signature, info }) => {
      if (info?.confirmations === "max" && !state[signature])
        fetchSignatures.add(signature);
      else if (info?.confirmations !== "max" && state[signature])
        removeSignatures.add(signature);
    });

    const removeList: string[] = [];
    removeSignatures.forEach(s => removeList.push(s));
    dispatch({ type: ActionType.Remove, signatures: removeList });

    const fetchList: string[] = [];
    fetchSignatures.forEach(s => fetchList.push(s));
    dispatch({ type: ActionType.Add, signatures: fetchList });
    fetchSignatures.forEach(signature => {
      fetchDetails(dispatch, signature, url);
    });
  }, [transactions]); // eslint-disable-line react-hooks/exhaustive-deps

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
    signature
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
