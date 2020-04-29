import React from "react";
import {
  TransactionSignature,
  Connection,
  SystemProgram,
  Account,
  SignatureResult
} from "@solana/web3.js";
import { findGetParameter, findPathSegment } from "../../utils/url";
import { useCluster, ClusterStatus } from "../cluster";
import {
  DetailsProvider,
  StateContext as DetailsStateContext,
  DispatchContext as DetailsDispatchContext
} from "./details";
import base58 from "bs58";
import {
  useAccountsDispatch,
  fetchAccountInfo,
  Dispatch as AccountsDispatch,
  ActionType as AccountsActionType
} from "../accounts";

export enum FetchStatus {
  Fetching,
  FetchFailed,
  Fetched
}

enum Source {
  Url,
  Input
}

export type Confirmations = number | "max";

export interface TransactionStatus {
  slot: number;
  result: SignatureResult;
  confirmations: Confirmations;
}

export interface TransactionState {
  id: number;
  source: Source;
  fetchStatus: FetchStatus;
  signature: TransactionSignature;
  transactionStatus?: TransactionStatus;
}

type Transactions = { [signature: string]: TransactionState };
interface State {
  idCounter: number;
  selected?: TransactionSignature;
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
  fetchStatus: FetchStatus;
  transactionStatus?: TransactionStatus;
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
      return { ...state, selected: tx.signature };
    }
    case ActionType.InputSignature: {
      if (!!state.transactions[action.signature]) return state;

      const nextId = state.idCounter + 1;
      const transactions = {
        ...state.transactions,
        [action.signature]: {
          id: nextId,
          source: Source.Input,
          signature: action.signature,
          fetchStatus: FetchStatus.Fetching
        }
      };
      return { ...state, transactions, idCounter: nextId };
    }
    case ActionType.UpdateStatus: {
      let transaction = state.transactions[action.signature];
      if (transaction) {
        transaction = {
          ...transaction,
          fetchStatus: action.fetchStatus
        };
        if (action.transactionStatus) {
          transaction.transactionStatus = action.transactionStatus;
        }
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
  "/tx",
  "/txs",
  "/txn",
  "/txns",
  "/transaction",
  "/transactions"
];

function urlSignatures(): Array<string> {
  const signatures: Array<string> = [];

  TX_PATHS.forEach(path => {
    const name = path.slice(1);
    const params = findGetParameter(name)?.split(",") || [];
    const segments = findPathSegment(name)?.split(",") || [];
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
      const nextId = idCounter + 1;
      transactions[signature] = {
        id: nextId,
        source: Source.Url,
        signature,
        fetchStatus: FetchStatus.Fetching
      };
      idCounter++;
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
        <DetailsProvider>{children}</DetailsProvider>
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
    signature,
    fetchStatus: FetchStatus.Fetching
  });

  let fetchStatus;
  let transactionStatus: TransactionStatus | undefined;
  try {
    const { value } = await new Connection(url).getSignatureStatus(signature, {
      searchTransactionHistory: true
    });

    if (value !== null) {
      let confirmations: Confirmations;
      if (typeof value.confirmations === "number") {
        confirmations = value.confirmations;
      } else {
        confirmations = "max";
      }

      transactionStatus = {
        slot: value.slot,
        confirmations,
        result: { err: value.err }
      };
    }
    fetchStatus = FetchStatus.Fetched;
  } catch (error) {
    console.error("Failed to fetch transaction status", error);
    fetchStatus = FetchStatus.FetchFailed;
  }
  dispatch({
    type: ActionType.UpdateStatus,
    signature,
    fetchStatus,
    transactionStatus
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

export function useDetailsDispatch() {
  const context = React.useContext(DetailsDispatchContext);
  if (!context) {
    throw new Error(
      `useDetailsDispatch must be used within a TransactionsProvider`
    );
  }
  return context;
}

export function useDetails(signature: TransactionSignature) {
  const context = React.useContext(DetailsStateContext);
  if (!context) {
    throw new Error(`useDetails must be used within a TransactionsProvider`);
  }
  return context[signature];
}
