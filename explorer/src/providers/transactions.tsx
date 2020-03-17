import React from "react";
import { TransactionSignature, Connection } from "@solana/web3.js";
import { findGetParameter } from "../utils";
import { useNetwork } from "../providers/network";

export enum Status {
  Checking,
  CheckFailed,
  Success,
  Failure,
  Pending
}

export interface Transaction {
  id: number;
  status: Status;
  recent: boolean;
  signature: TransactionSignature;
}

type Transactions = { [id: number]: Transaction };
interface State {
  idCounter: number;
  transactions: Transactions;
}

interface UpdateStatus {
  id: number;
  status: Status;
}

type Action = UpdateStatus;
type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  let transaction = state.transactions[action.id];
  if (transaction) {
    transaction = { ...transaction, status: action.status };
    const transactions = { ...state.transactions, [action.id]: transaction };
    return { ...state, transactions };
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
        recent: true,
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
    const connection = new Connection(url);
    Object.values(state.transactions).forEach(tx => {
      checkTransactionStatus(dispatch, tx, connection);
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
  transaction: Transaction,
  connection: Connection
) {
  const id = transaction.id;
  dispatch({
    status: Status.Checking,
    id
  });

  let status;
  try {
    const signatureStatus = await connection.getSignatureStatus(
      transaction.signature
    );

    if (signatureStatus === null) {
      status = Status.Pending;
    } else if ("Ok" in signatureStatus) {
      status = Status.Success;
    } else {
      status = Status.Failure;
    }
  } catch (error) {
    console.error("Failed to check transaction status", error);
    status = Status.CheckFailed;
  }
  dispatch({ status, id });
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

export function useTransactionsDispatch() {
  const context = React.useContext(DispatchContext);
  if (!context) {
    throw new Error(
      `useTransactionsDispatch must be used within a TransactionsProvider`
    );
  }
  return context;
}
