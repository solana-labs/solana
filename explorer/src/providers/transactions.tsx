import React from "react";
import { TransactionSignature, Connection } from "@solana/web3.js";
import { findGetParameter } from "../utils";
import { useNetwork } from "../providers/network";

export enum Status {
  Checking,
  CheckFailed,
  Success,
  Failure,
  Missing
}

enum Source {
  Url,
  Input
}

export interface Transaction {
  id: number;
  status: Status;
  source: Source;
  signature: TransactionSignature;
}

type Transactions = { [id: number]: Transaction };
interface State {
  idCounter: number;
  transactions: Transactions;
}

export enum ActionType {
  UpdateStatus,
  InputSignature
}

interface UpdateStatus {
  type: ActionType.UpdateStatus;
  id: number;
  status: Status;
}

interface InputSignature {
  type: ActionType.InputSignature;
  signature: TransactionSignature;
}

type Action = UpdateStatus | InputSignature;
type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.InputSignature: {
      const idCounter = state.idCounter + 1;
      const transactions = {
        ...state.transactions,
        [idCounter]: {
          id: idCounter,
          status: Status.Checking,
          source: Source.Input,
          signature: action.signature
        }
      };
      return { ...state, transactions, idCounter };
    }
    case ActionType.UpdateStatus: {
      let transaction = state.transactions[action.id];
      if (transaction) {
        transaction = { ...transaction, status: action.status };
        const transactions = {
          ...state.transactions,
          [action.id]: transaction
        };
        return { ...state, transactions };
      }
      break;
    }
  }
  return state;
}

function initState(): State {
  let idCounter = 0;
  const signatures = findGetParameter("txs")?.split(",") || [];
  const transactions = signatures.reduce(
    (transactions: Transactions, signature) => {
      const id = ++idCounter;
      transactions[id] = {
        id,
        status: Status.Checking,
        source: Source.Url,
        signature
      };
      return transactions;
    },
    {}
  );
  return { idCounter, transactions };
}

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type TransactionsProviderProps = { children: React.ReactNode };
export function TransactionsProvider({ children }: TransactionsProviderProps) {
  const [state, dispatch] = React.useReducer(reducer, undefined, initState);

  const { status, url } = useNetwork();

  // Check transaction statuses on startup and whenever network updates
  React.useEffect(() => {
    Object.values(state.transactions).forEach(tx => {
      checkTransactionStatus(dispatch, tx.id, tx.signature, url);
    });
  }, [status, url]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        {children}
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

export async function checkTransactionStatus(
  dispatch: Dispatch,
  id: number,
  signature: TransactionSignature,
  url: string
) {
  dispatch({
    type: ActionType.UpdateStatus,
    status: Status.Checking,
    id
  });

  let status;
  try {
    const signatureStatus = await new Connection(url).getSignatureStatus(
      signature
    );

    if (signatureStatus === null) {
      status = Status.Missing;
    } else if ("Ok" in signatureStatus.status) {
      status = Status.Success;
    } else {
      status = Status.Failure;
    }
  } catch (error) {
    console.error("Failed to check transaction status", error);
    status = Status.CheckFailed;
  }
  dispatch({ type: ActionType.UpdateStatus, status, id });
}

export function useTransactions() {
  const context = React.useContext(StateContext);
  if (!context) {
    throw new Error(
      `useTransactions must be used within a TransactionsProvider`
    );
  }
  return {
    idCounter: context.idCounter,
    transactions: Object.values(context.transactions).sort((a, b) =>
      a.id <= b.id ? 1 : -1
    )
  };
}

export function useTransactionsDispatch() {
  const context = React.useContext(DispatchContext);
  if (!context) {
    throw new Error(
      `useTransactionsDispatch must be used within a TransactionsProvider`
    );
  }
  return context;
}
