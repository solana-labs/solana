import React from "react";
import {
  TransactionSignature,
  Connection,
  SystemProgram,
  Account,
  SignatureResult
} from "@solana/web3.js";
import { findGetParameter } from "../../utils/url";
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
import { useLocation } from "react-router-dom";

export enum FetchStatus {
  Fetching,
  FetchFailed,
  Fetched
}

export enum Source {
  Url,
  Input,
  Test
}

export type Confirmations = number | "max";

export interface TransactionStatusInfo {
  slot: number;
  result: SignatureResult;
  confirmations: Confirmations;
}

export interface TransactionStatus {
  id: number;
  source: Source;
  fetchStatus: FetchStatus;
  signature: TransactionSignature;
  info?: TransactionStatusInfo;
}

type Transactions = { [signature: string]: TransactionStatus };
interface State {
  idCounter: number;
  selected?: TransactionSignature;
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
  source: Source;
}

type Action = UpdateStatus | FetchSignature;

type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.FetchSignature: {
      if (!!state.transactions[action.signature]) return state;

      const nextId = state.idCounter + 1;
      const transactions = {
        ...state.transactions,
        [action.signature]: {
          id: nextId,
          source: action.source,
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
          fetchStatus: action.fetchStatus,
          info: action.info
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

export const TX_ALIASES = ["tx", "txn", "transaction"];

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type TransactionsProviderProps = { children: React.ReactNode };
export function TransactionsProvider({ children }: TransactionsProviderProps) {
  const [state, dispatch] = React.useReducer(reducer, {
    idCounter: 0,
    transactions: {}
  });

  const { status, url } = useCluster();
  const accountsDispatch = useAccountsDispatch();
  const search = useLocation().search;

  // Check transaction statuses whenever cluster updates
  React.useEffect(() => {
    if (status !== ClusterStatus.Connected) return;

    Object.keys(state.transactions).forEach(signature => {
      dispatch({
        type: ActionType.FetchSignature,
        signature,
        source: Source.Url
      });
      checkTransactionStatus(dispatch, signature, url);
    });

    // Create a test transaction
    if (findGetParameter("test") !== null) {
      createTestTransaction(dispatch, accountsDispatch, url);
    }
  }, [status, url]); // eslint-disable-line react-hooks/exhaustive-deps

  // Check for transactions in the url params
  React.useEffect(() => {
    TX_ALIASES.flatMap(key =>
      (findGetParameter(key)?.split(",") || []).concat(
        findGetParameter(key + "s")?.split(",") || []
      )
    )
      .flatMap(paramValue => paramValue?.split(",") || [])
      .filter(signature => !state.transactions[signature])
      .forEach(signature => {
        dispatch({
          type: ActionType.FetchSignature,
          signature,
          source: Source.Url
        });
        checkTransactionStatus(dispatch, signature, url);
      });
  }, [search]); // eslint-disable-line react-hooks/exhaustive-deps

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
    dispatch({
      type: ActionType.FetchSignature,
      signature,
      source: Source.Test
    });
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
    dispatch({
      type: ActionType.FetchSignature,
      signature,
      source: Source.Test
    });
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
  let info: TransactionStatusInfo | undefined;
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

      info = {
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
