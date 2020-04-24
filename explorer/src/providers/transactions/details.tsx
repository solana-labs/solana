import React from "react";
import {
  Connection,
  TransactionSignature,
  ConfirmedTransaction
} from "@solana/web3.js";
import { useCluster, ClusterStatus } from "../cluster";
import { useTransactions } from "./index";

export enum Status {
  Checking,
  CheckFailed,
  NotFound,
  Found
}

export interface Details {
  status: Status;
  transaction: ConfirmedTransaction | null;
}

type State = { [signature: string]: Details };

export enum ActionType {
  Update,
  Add
}

interface Update {
  type: ActionType.Update;
  signature: string;
  status: Status;
  transaction: ConfirmedTransaction | null;
}

interface Add {
  type: ActionType.Add;
  signatures: TransactionSignature[];
}

type Action = Update | Add;
type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.Add: {
      if (action.signatures.length === 0) return state;
      const details = { ...state };
      action.signatures.forEach(signature => {
        if (!details[signature]) {
          details[signature] = {
            status: Status.Checking,
            transaction: null
          };
        }
      });
      return details;
    }

    case ActionType.Update: {
      let details = state[action.signature];
      if (details) {
        details = {
          ...details,
          status: action.status
        };
        if (action.transaction !== null) {
          details.transaction = action.transaction;
        }
        return {
          ...state,
          ...{
            [action.signature]: details
          }
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
  const { status, url } = useCluster();

  // Filter blocks for current transaction slots
  React.useEffect(() => {
    if (status !== ClusterStatus.Connected) return;

    const fetchSignatures = new Set<string>();
    transactions.forEach(tx => {
      if (tx.slot && tx.confirmations === "max" && !state[tx.signature])
        fetchSignatures.add(tx.signature);
    });

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
    status: Status.Checking,
    transaction: null,
    signature
  });

  let status;
  let transaction = null;
  try {
    transaction = await new Connection(url).getConfirmedTransaction(signature);
    if (transaction) {
      status = Status.Found;
    } else {
      status = Status.NotFound;
    }
  } catch (error) {
    console.error("Failed to fetch confirmed transaction", error);
    status = Status.CheckFailed;
  }
  dispatch({ type: ActionType.Update, status, signature, transaction });
}
