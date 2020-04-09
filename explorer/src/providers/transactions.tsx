import React from "react";
import {
  TransactionSignature,
  Connection,
  SystemProgram,
  Account
} from "@solana/web3.js";
import { findGetParameter, findPathSegment } from "../utils";
import { useCluster, ClusterStatus } from "../providers/cluster";
import base58 from "bs58";
import {
  useAccountsDispatch,
  fetchAccountInfo,
  Dispatch as AccountsDispatch,
  ActionType as AccountsActionType
} from "./accounts";

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

export type Confirmations = number | "max";

export interface Transaction {
  id: number;
  status: Status;
  source: Source;
  slot?: number;
  confirmations?: Confirmations;
  signature: TransactionSignature;
}

export interface Selected {
  slot: number;
  signature: TransactionSignature;
}

type Transactions = { [signature: string]: Transaction };
interface State {
  idCounter: number;
  selected?: Selected;
  transactions: Transactions;
}

export enum ActionType {
  UpdateStatus,
  InputSignature,
  Select,
  Deselect
}

interface SelectTransaction {
  type: ActionType.Select;
  signature: TransactionSignature;
}

interface DeselectTransaction {
  type: ActionType.Deselect;
}

interface UpdateStatus {
  type: ActionType.UpdateStatus;
  signature: TransactionSignature;
  status: Status;
  slot?: number;
  confirmations?: Confirmations;
}

interface InputSignature {
  type: ActionType.InputSignature;
  signature: TransactionSignature;
}

type Action =
  | UpdateStatus
  | InputSignature
  | SelectTransaction
  | DeselectTransaction;
type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.Deselect: {
      return { ...state, selected: undefined };
    }
    case ActionType.Select: {
      const tx = state.transactions[action.signature];
      if (!tx.slot) return state;
      const selected = {
        slot: tx.slot,
        signature: tx.signature
      };
      return { ...state, selected };
    }
    case ActionType.InputSignature: {
      if (!!state.transactions[action.signature]) return state;

      const idCounter = state.idCounter + 1;
      const transactions = {
        ...state.transactions,
        [action.signature]: {
          id: idCounter,
          status: Status.Checking,
          source: Source.Input,
          signature: action.signature
        }
      };
      return { ...state, transactions, idCounter };
    }
    case ActionType.UpdateStatus: {
      let transaction = state.transactions[action.signature];
      if (transaction) {
        transaction = {
          ...transaction,
          status: action.status,
          slot: action.slot,
          confirmations: action.confirmations
        };
        const transactions = {
          ...state.transactions,
          [action.signature]: transaction
        };
        return { ...state, transactions };
      }
      break;
    }
  }
  return state;
}

export const TX_PATHS = [
  "tx",
  "txs",
  "txn",
  "txns",
  "transaction",
  "transactions"
];

function urlSignatures(): Array<string> {
  const signatures: Array<string> = [];

  TX_PATHS.forEach(path => {
    const params = findGetParameter(path)?.split(",") || [];
    const segments = findPathSegment(path)?.split(",") || [];
    signatures.push(...params);
    signatures.push(...segments);
  });

  return signatures.filter(s => s.length > 0);
}

function initState(): State {
  let idCounter = 0;
  const signatures = urlSignatures();
  const transactions = signatures.reduce(
    (transactions: Transactions, signature) => {
      if (!!transactions[signature]) return transactions;
      idCounter++;
      transactions[signature] = {
        id: idCounter,
        signature,
        status: Status.Checking,
        source: Source.Url
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

  const { status, url } = useCluster();
  const accountsDispatch = useAccountsDispatch();

  // Check transaction statuses on startup and whenever cluster updates
  React.useEffect(() => {
    if (status !== ClusterStatus.Connected) return;

    // Create a test transaction
    if (findGetParameter("test") !== null) {
      createTestTransaction(dispatch, accountsDispatch, url);
    }

    Object.keys(state.transactions).forEach(signature => {
      checkTransactionStatus(dispatch, signature, url);
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

async function createTestTransaction(
  dispatch: Dispatch,
  accountsDispatch: AccountsDispatch,
  url: string
) {
  const testKey = process.env.REACT_APP_TEST_KEY;
  let testAccount = new Account();
  if (testKey) {
    testAccount = new Account(base58.decode(testKey));
  }

  try {
    const connection = new Connection(url, "recent");
    const signature = await connection.requestAirdrop(
      testAccount.publicKey,
      100000,
      "recent"
    );
    dispatch({ type: ActionType.InputSignature, signature });
    checkTransactionStatus(dispatch, signature, url);
    accountsDispatch({
      type: AccountsActionType.Input,
      pubkey: testAccount.publicKey
    });
    fetchAccountInfo(accountsDispatch, testAccount.publicKey.toBase58(), url);
  } catch (error) {
    console.error("Failed to create test success transaction", error);
  }

  try {
    const connection = new Connection(url, "recent");
    const tx = SystemProgram.transfer({
      fromPubkey: testAccount.publicKey,
      toPubkey: testAccount.publicKey,
      lamports: 1
    });
    const signature = await connection.sendTransaction(tx, testAccount);
    dispatch({ type: ActionType.InputSignature, signature });
    checkTransactionStatus(dispatch, signature, url);
  } catch (error) {
    console.error("Failed to create test failure transaction", error);
  }
}

export async function checkTransactionStatus(
  dispatch: Dispatch,
  signature: TransactionSignature,
  url: string
) {
  dispatch({
    type: ActionType.UpdateStatus,
    status: Status.Checking,
    signature
  });

  let status;
  let slot;
  let confirmations: Confirmations | undefined;
  try {
    const { value } = await new Connection(url).getSignatureStatus(signature, {
      searchTransactionHistory: true
    });

    if (value === null) {
      status = Status.Missing;
    } else {
      slot = value.slot;
      if (typeof value.confirmations === "number") {
        confirmations = value.confirmations;
      } else {
        confirmations = "max";
      }

      if (value.err) {
        status = Status.Failure;
      } else {
        status = Status.Success;
      }
    }
  } catch (error) {
    console.error("Failed to check transaction status", error);
    status = Status.CheckFailed;
  }
  dispatch({
    type: ActionType.UpdateStatus,
    status,
    slot,
    confirmations,
    signature
  });
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
    selected: context.selected,
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
