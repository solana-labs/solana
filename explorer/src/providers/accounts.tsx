import React from "react";
import {
  PublicKey,
  Connection,
  TransactionSignature,
  TransactionError,
  SignatureStatus
} from "@solana/web3.js";
import { findGetParameter, findPathSegment } from "../utils/url";
import { useCluster, ClusterStatus } from "./cluster";

export enum Status {
  Checking,
  CheckFailed,
  FetchingHistory,
  HistoryFailed,
  NotFound,
  Success
}

export type History = Map<
  number,
  Map<TransactionSignature, TransactionError | null>
>;

export interface Details {
  executable: boolean;
  owner: PublicKey;
  space: number;
}

export interface Account {
  id: number;
  pubkey: PublicKey;
  status: Status;
  lamports?: number;
  details?: Details;
  history?: History;
}

type Accounts = { [address: string]: Account };
interface State {
  idCounter: number;
  accounts: Accounts;
  selected?: string;
}

export enum ActionType {
  Update,
  Input,
  Select
}

interface Update {
  type: ActionType.Update;
  address: string;
  data: {
    status: Status;
    lamports?: number;
    details?: Details;
    history?: History;
  };
}

interface Input {
  type: ActionType.Input;
  pubkey: PublicKey;
}

interface Select {
  type: ActionType.Select;
  address?: string;
}

type Action = Update | Input | Select;
export type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.Input: {
      const address = action.pubkey.toBase58();
      if (!!state.accounts[address]) return state;
      const idCounter = state.idCounter + 1;
      const accounts = {
        ...state.accounts,
        [address]: {
          id: idCounter,
          status: Status.Checking,
          pubkey: action.pubkey
        }
      };
      return { ...state, accounts, idCounter };
    }

    case ActionType.Select: {
      return { ...state, selected: action.address };
    }

    case ActionType.Update: {
      let account = state.accounts[action.address];
      if (account) {
        account = {
          ...account,
          ...action.data
        };
        const accounts = {
          ...state.accounts,
          [action.address]: account
        };
        return { ...state, accounts };
      }
      break;
    }
  }
  return state;
}

export const ACCOUNT_PATHS = [
  "/account",
  "/accounts",
  "/address",
  "/addresses"
];

function urlAddresses(): Array<string> {
  const addresses: Array<string> = [];

  ACCOUNT_PATHS.forEach(path => {
    const name = path.slice(1);
    const params = findGetParameter(name)?.split(",") || [];
    const segments = findPathSegment(name)?.split(",") || [];
    addresses.push(...params);
    addresses.push(...segments);
  });

  return addresses.filter(a => a.length > 0);
}

function initState(): State {
  let idCounter = 0;
  const addresses = urlAddresses();
  const accounts = addresses.reduce((accounts: Accounts, address) => {
    if (!!accounts[address]) return accounts;
    try {
      const pubkey = new PublicKey(address);
      const id = ++idCounter;
      accounts[address] = {
        id,
        status: Status.Checking,
        pubkey
      };
    } catch (err) {
      // TODO display to user
      console.error(err);
    }
    return accounts;
  }, {});
  return { idCounter, accounts };
}

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type AccountsProviderProps = { children: React.ReactNode };
export function AccountsProvider({ children }: AccountsProviderProps) {
  const [state, dispatch] = React.useReducer(reducer, undefined, initState);

  const { status, url } = useCluster();

  // Check account statuses on startup and whenever cluster updates
  React.useEffect(() => {
    if (status !== ClusterStatus.Connected) return;

    Object.keys(state.accounts).forEach(address => {
      fetchAccountInfo(dispatch, address, url);
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

export async function fetchAccountInfo(
  dispatch: Dispatch,
  address: string,
  url: string
) {
  dispatch({
    type: ActionType.Update,
    address,
    data: {
      status: Status.Checking
    }
  });

  let status;
  let details;
  let lamports;
  try {
    const result = await new Connection(url, "recent").getAccountInfo(
      new PublicKey(address)
    );
    if (result === null) {
      lamports = 0;
      status = Status.NotFound;
    } else {
      lamports = result.lamports;
      details = {
        space: result.data.length,
        executable: result.executable,
        owner: result.owner
      };
      status = Status.FetchingHistory;
      fetchAccountHistory(dispatch, address, url);
    }
  } catch (error) {
    console.error("Failed to fetch account info", error);
    status = Status.CheckFailed;
  }
  const data = { status, lamports, details };
  dispatch({ type: ActionType.Update, data, address });
}

async function fetchAccountHistory(
  dispatch: Dispatch,
  address: string,
  url: string
) {
  let history;
  let status;
  try {
    const connection = new Connection(url);
    const currentSlot = await connection.getSlot();
    const signatures = await connection.getConfirmedSignaturesForAddress(
      new PublicKey(address),
      Math.max(0, currentSlot - 10000 + 1),
      currentSlot
    );

    let statuses: (SignatureStatus | null)[] = [];
    if (signatures.length > 0) {
      statuses = (
        await connection.getSignatureStatuses(signatures, {
          searchTransactionHistory: true
        })
      ).value;
    }

    history = new Map();
    for (let i = 0; i < statuses.length; i++) {
      const status = statuses[i];
      if (!status) continue;
      let slotSignatures = history.get(status.slot);
      if (!slotSignatures) {
        slotSignatures = new Map();
        history.set(status.slot, slotSignatures);
      }
      slotSignatures.set(signatures[i], status.err);
    }
    status = Status.Success;
  } catch (error) {
    console.error("Failed to fetch account history", error);
    status = Status.HistoryFailed;
  }
  const data = { status, history };
  dispatch({ type: ActionType.Update, data, address });
}

export function useAccounts() {
  const context = React.useContext(StateContext);
  if (!context) {
    throw new Error(`useAccounts must be used within a AccountsProvider`);
  }
  return {
    idCounter: context.idCounter,
    accounts: Object.values(context.accounts).sort((a, b) =>
      a.id <= b.id ? 1 : -1
    )
  };
}

export function useSelectedAccount() {
  const context = React.useContext(StateContext);
  if (!context) {
    throw new Error(
      `useSelectedAccount must be used within a AccountsProvider`
    );
  }

  if (!context.selected) return undefined;
  return context.accounts[context.selected];
}

export function useAccountsDispatch() {
  const context = React.useContext(DispatchContext);
  if (!context) {
    throw new Error(
      `useAccountsDispatch must be used within a AccountsProvider`
    );
  }
  return context;
}
