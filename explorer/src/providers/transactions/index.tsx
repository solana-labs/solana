import React from "react";
import {
  TransactionSignature,
  Connection,
  SystemProgram,
  Account,
  SignatureResult,
  PublicKey,
  sendAndConfirmTransaction
} from "@solana/web3.js";
import { useQuery } from "../../utils/url";
import { useCluster, Cluster, ClusterStatus } from "../cluster";
import {
  DetailsProvider,
  StateContext as DetailsStateContext,
  DispatchContext as DetailsDispatchContext
} from "./details";
import base58 from "bs58";
import { useFetchAccountInfo } from "../accounts";

export enum FetchStatus {
  Fetching,
  FetchFailed,
  Fetched
}

export type Confirmations = number | "max";

export interface TransactionStatusInfo {
  slot: number;
  result: SignatureResult;
  timestamp: number | null;
  confirmations: Confirmations;
}

export interface TransactionStatus {
  id: number;
  fetchStatus: FetchStatus;
  signature: TransactionSignature;
  info?: TransactionStatusInfo;
}

type Transactions = { [signature: string]: TransactionStatus };
interface State {
  idCounter: number;
  transactions: Transactions;
}

export enum ActionType {
  UpdateStatus,
  FetchSignature
}

interface UpdateStatus {
  type: ActionType.UpdateStatus;
  signature: TransactionSignature;
  fetchStatus: FetchStatus;
  info?: TransactionStatusInfo;
}

interface FetchSignature {
  type: ActionType.FetchSignature;
  signature: TransactionSignature;
}

type Action = UpdateStatus | FetchSignature;

type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.FetchSignature: {
      const transaction = state.transactions[action.signature];
      if (transaction) {
        const transactions = {
          ...state.transactions,
          [action.signature]: {
            ...transaction,
            fetchStatus: FetchStatus.Fetching,
            info: undefined
          }
        };
        return { ...state, transactions };
      } else {
        const nextId = state.idCounter + 1;
        const transactions = {
          ...state.transactions,
          [action.signature]: {
            id: nextId,
            signature: action.signature,
            fetchStatus: FetchStatus.Fetching
          }
        };
        return { ...state, transactions, idCounter: nextId };
      }
    }

    case ActionType.UpdateStatus: {
      const transaction = state.transactions[action.signature];
      if (transaction) {
        const transactions = {
          ...state.transactions,
          [action.signature]: {
            ...transaction,
            fetchStatus: action.fetchStatus,
            info: action.info
          }
        };
        return { ...state, transactions };
      }
      break;
    }
  }
  return state;
}

export const TX_ALIASES = ["tx", "txn", "transaction"];

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type TransactionsProviderProps = { children: React.ReactNode };
export function TransactionsProvider({ children }: TransactionsProviderProps) {
  const [state, dispatch] = React.useReducer(reducer, {
    idCounter: 0,
    transactions: {}
  });

  const { cluster, status: clusterStatus, url } = useCluster();
  const fetchAccount = useFetchAccountInfo();
  const query = useQuery();
  const testFlag = query.get("test");

  // Check transaction statuses whenever cluster updates
  React.useEffect(() => {
    Object.keys(state.transactions).forEach(signature => {
      fetchTransactionStatus(dispatch, signature, url, clusterStatus);
    });

    // Create a test transaction
    if (cluster === Cluster.Devnet && testFlag !== null) {
      createTestTransaction(dispatch, fetchAccount, url, clusterStatus);
    }
  }, [testFlag, cluster, clusterStatus, url]); // eslint-disable-line react-hooks/exhaustive-deps

  // Check for transactions in the url params
  const values = TX_ALIASES.flatMap(key => [
    query.get(key),
    query.get(key + "s")
  ]);
  React.useEffect(() => {
    values
      .filter((value): value is string => value !== null)
      .flatMap(value => value.split(","))
      // Remove duplicates
      .filter((item, pos, self) => self.indexOf(item) === pos)
      .filter(signature => !state.transactions[signature])
      .forEach(signature => {
        fetchTransactionStatus(dispatch, signature, url, clusterStatus);
      });
  }, [values.toString()]); // eslint-disable-line react-hooks/exhaustive-deps

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
  fetchAccount: (pubkey: PublicKey) => void,
  url: string,
  clusterStatus: ClusterStatus
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
    fetchTransactionStatus(dispatch, signature, url, clusterStatus);
    fetchAccount(testAccount.publicKey);
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
    const signature = await sendAndConfirmTransaction(
      connection,
      tx,
      [testAccount],
      1
    );
    fetchTransactionStatus(dispatch, signature, url, clusterStatus);
  } catch (error) {
    console.error("Failed to create test failure transaction", error);
  }
}

export async function fetchTransactionStatus(
  dispatch: Dispatch,
  signature: TransactionSignature,
  url: string,
  status: ClusterStatus
) {
  dispatch({
    type: ActionType.FetchSignature,
    signature
  });

  // We will auto-refetch when status is no longer connecting
  if (status === ClusterStatus.Connecting) return;

  let fetchStatus;
  let info: TransactionStatusInfo | undefined;
  try {
    const connection = new Connection(url);
    const { value } = await connection.getSignatureStatus(signature, {
      searchTransactionHistory: true
    });

    if (value !== null) {
      let timestamp = await connection.getBlockTime(value.slot);
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
    info
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
    transactions: Object.values(context.transactions).sort((a, b) =>
      a.id <= b.id ? 1 : -1
    )
  };
}

export function useTransactionStatus(signature: TransactionSignature) {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useTransactionStatus must be used within a TransactionsProvider`
    );
  }

  return context.transactions[signature];
}

export function useTransactionDetails(signature: TransactionSignature) {
  const context = React.useContext(DetailsStateContext);

  if (!context) {
    throw new Error(
      `useTransactionDetails must be used within a TransactionsProvider`
    );
  }

  return context[signature];
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

export function useFetchTransactionStatus() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchTransactionStatus must be used within a TransactionsProvider`
    );
  }

  const { url, status } = useCluster();
  return (signature: TransactionSignature) => {
    fetchTransactionStatus(dispatch, signature, url, status);
  };
}
