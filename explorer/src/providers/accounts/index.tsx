import React from "react";
import { StakeAccount } from "solana-sdk-wasm";
import { PublicKey, Connection, StakeProgram } from "@solana/web3.js";
import { useCluster, ClusterStatus } from "../cluster";
import { HistoryProvider } from "./history";
export { useAccountHistory } from "./history";

export enum FetchStatus {
  Fetching,
  FetchFailed,
  Fetched,
}

export interface Details {
  executable: boolean;
  owner: PublicKey;
  space: number;
  data?: StakeAccount;
}

export interface Account {
  pubkey: PublicKey;
  status: FetchStatus;
  lamports?: number;
  details?: Details;
}

type Accounts = { [address: string]: Account };
interface State {
  accounts: Accounts;
  lastFetchedAddress: string | undefined;
}

export enum ActionType {
  Update,
  Fetch,
  Clear,
}

interface Update {
  type: ActionType.Update;
  pubkey: PublicKey;
  data: {
    status: FetchStatus;
    lamports?: number;
    details?: Details;
  };
}

interface Fetch {
  type: ActionType.Fetch;
  pubkey: PublicKey;
}

interface Clear {
  type: ActionType.Clear;
}

type Action = Update | Fetch | Clear;
type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.Fetch: {
      const address = action.pubkey.toBase58();
      const account = state.accounts[address];
      if (account) {
        const accounts = {
          ...state.accounts,
          [address]: {
            pubkey: account.pubkey,
            status: FetchStatus.Fetching,
          },
        };
        return { ...state, accounts, lastFetchedAddress: address };
      } else {
        const accounts = {
          ...state.accounts,
          [address]: {
            status: FetchStatus.Fetching,
            pubkey: action.pubkey,
          },
        };
        return { ...state, accounts, lastFetchedAddress: address };
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
            ...action.data,
          },
        };
        return { ...state, accounts };
      }
      break;
    }

    case ActionType.Clear: {
      return {
        ...state,
        accounts: {},
      };
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
    accounts: {},
    lastFetchedAddress: undefined,
  });

  // Check account statuses on startup and whenever cluster updates
  const { status, url } = useCluster();
  React.useEffect(() => {
    if (status === ClusterStatus.Connecting) {
      dispatch({ type: ActionType.Clear });
    } else if (status === ClusterStatus.Connected && state.lastFetchedAddress) {
      fetchAccountInfo(dispatch, new PublicKey(state.lastFetchedAddress), url);
    }
  }, [status, url]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        <HistoryProvider>{children}</HistoryProvider>
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

async function fetchAccountInfo(
  dispatch: Dispatch,
  pubkey: PublicKey,
  url: string
) {
  dispatch({
    type: ActionType.Fetch,
    pubkey,
  });

  let fetchStatus;
  let details;
  let lamports;
  try {
    const result = await new Connection(url, "recent").getAccountInfo(pubkey);
    if (result === null) {
      lamports = 0;
    } else {
      lamports = result.lamports;
      let data = undefined;

      // Only save data in memory if we can decode it
      if (result.owner.equals(StakeProgram.programId)) {
        try {
          const wasm = await import("solana-sdk-wasm");
          data = wasm.StakeAccount.fromAccountData(result.data);
        } catch (err) {
          console.error("Unexpected error loading wasm", err);
          // TODO store error state in Account info
        }
      }

      details = {
        space: result.data.length,
        executable: result.executable,
        owner: result.owner,
        data,
      };
    }
    fetchStatus = FetchStatus.Fetched;
  } catch (error) {
    console.error("Failed to fetch account info", error);
    fetchStatus = FetchStatus.FetchFailed;
  }
  const data = { status: fetchStatus, lamports, details };
  dispatch({ type: ActionType.Update, data, pubkey });
}

export function useAccounts() {
  const context = React.useContext(StateContext);
  if (!context) {
    throw new Error(`useAccounts must be used within a AccountsProvider`);
  }
  return context;
}

export function useAccountInfo(address: string) {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useAccountInfo must be used within a AccountsProvider`);
  }

  return context.accounts[address];
}

export function useFetchAccountInfo() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchAccountInfo must be used within a AccountsProvider`
    );
  }

  const { url } = useCluster();
  return (pubkey: PublicKey) => {
    fetchAccountInfo(dispatch, pubkey, url);
  };
}
