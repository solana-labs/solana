import React from "react";
import {
  PublicKey,
  Connection,
  TransactionSignature,
  TransactionError,
  SignatureStatus,
  StakeProgram
} from "@solana/web3.js";
import { useQuery } from "../utils/url";
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
  data?: Buffer;
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
}

export enum ActionType {
  Update,
  Fetch
}

interface Update {
  type: ActionType.Update;
  pubkey: PublicKey;
  data: {
    status: Status;
    lamports?: number;
    details?: Details;
    history?: History;
  };
}

interface Fetch {
  type: ActionType.Fetch;
  pubkey: PublicKey;
}

type Action = Update | Fetch;
export type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.Fetch: {
      const address = action.pubkey.toBase58();
      const account = state.accounts[address];
      if (account) {
        const accounts = {
          ...state.accounts,
          [address]: {
            id: account.id,
            pubkey: account.pubkey,
            status: Status.Checking
          }
        };
        return { ...state, accounts };
      } else {
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
    }

    case ActionType.Update: {
      const address = action.pubkey.toBase58();
      const account = state.accounts[address];
      if (account) {
        const accounts = {
          ...state.accounts,
          [address]: {
            ...account,
            ...action.data
          }
        };
        return { ...state, accounts };
      }
      break;
    }
  }
  return state;
}

export const ACCOUNT_ALIASES = ["account", "address"];
export const ACCOUNT_ALIASES_PLURAL = ["accounts", "addresses"];

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type AccountsProviderProps = { children: React.ReactNode };
export function AccountsProvider({ children }: AccountsProviderProps) {
  const [state, dispatch] = React.useReducer(reducer, {
    idCounter: 0,
    accounts: {}
  });

  const { status, url } = useCluster();

  // Check account statuses on startup and whenever cluster updates
  React.useEffect(() => {
    Object.keys(state.accounts).forEach(address => {
      fetchAccountInfo(dispatch, new PublicKey(address), url, status);
    });
  }, [status, url]); // eslint-disable-line react-hooks/exhaustive-deps

  const query = useQuery();
  const values = ACCOUNT_ALIASES.concat(ACCOUNT_ALIASES_PLURAL).map(key =>
    query.get(key)
  );
  React.useEffect(() => {
    values
      .filter((value): value is string => value !== null)
      .flatMap(value => value.split(","))
      // Remove duplicates
      .filter((item, pos, self) => self.indexOf(item) === pos)
      .filter(address => !state.accounts[address])
      .forEach(address => {
        try {
          fetchAccountInfo(dispatch, new PublicKey(address), url, status);
        } catch (err) {
          console.error(err);
          // TODO handle bad addresses
        }
      });
  }, [values.toString()]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        {children}
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

async function fetchAccountInfo(
  dispatch: Dispatch,
  pubkey: PublicKey,
  url: string,
  status: ClusterStatus
) {
  dispatch({
    type: ActionType.Fetch,
    pubkey
  });

  // We will auto-refetch when status is no longer connecting
  if (status === ClusterStatus.Connecting) return;

  let fetchStatus;
  let details;
  let lamports;
  try {
    const result = await new Connection(url, "recent").getAccountInfo(pubkey);
    if (result === null) {
      lamports = 0;
      fetchStatus = Status.NotFound;
    } else {
      lamports = result.lamports;
      let data = undefined;

      // Only save data in memory if we can decode it
      if (result.owner.equals(StakeProgram.programId)) {
        data = result.data;
      }

      details = {
        space: result.data.length,
        executable: result.executable,
        owner: result.owner,
        data
      };
      fetchStatus = Status.FetchingHistory;
      fetchAccountHistory(dispatch, pubkey, url);
    }
  } catch (error) {
    console.error("Failed to fetch account info", error);
    fetchStatus = Status.CheckFailed;
  }
  const data = { status: fetchStatus, lamports, details };
  dispatch({ type: ActionType.Update, data, pubkey });
}

async function fetchAccountHistory(
  dispatch: Dispatch,
  pubkey: PublicKey,
  url: string
) {
  dispatch({
    type: ActionType.Update,
    data: { status: Status.FetchingHistory },
    pubkey
  });

  let history;
  let status;
  try {
    const connection = new Connection(url);
    const currentSlot = await connection.getSlot();
    const signatures = await connection.getConfirmedSignaturesForAddress(
      pubkey,
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
  dispatch({ type: ActionType.Update, data, pubkey });
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

export function useAccountInfo(address: string) {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useAccountInfo must be used within a AccountsProvider`);
  }

  return context.accounts[address];
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

export function useFetchAccountInfo() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchAccountInfo must be used within a AccountsProvider`
    );
  }

  const { url, status } = useCluster();
  return (pubkey: PublicKey) => {
    fetchAccountInfo(dispatch, pubkey, url, status);
  };
}

export function useFetchAccountHistory() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchAccountHistory must be used within a AccountsProvider`
    );
  }

  const { url } = useCluster();
  return (pubkey: PublicKey) => {
    fetchAccountHistory(dispatch, pubkey, url);
  };
}
