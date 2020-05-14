import React from "react";
import { StakeAccount } from "solana-sdk-wasm";
import { PublicKey, Connection, StakeProgram } from "@solana/web3.js";
import { useQuery } from "../../utils/url";
import { useCluster, ClusterStatus } from "../cluster";
import { HistoryProvider } from "./history";
export { useAccountHistory } from "./history";

export enum FetchStatus {
  Fetching,
  FetchFailed,
  Fetched
}

export interface Details {
  executable: boolean;
  owner: PublicKey;
  space: number;
  data?: StakeAccount;
}

export interface Account {
  id: number;
  pubkey: PublicKey;
  status: FetchStatus;
  lamports?: number;
  details?: Details;
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
    status: FetchStatus;
    lamports?: number;
    details?: Details;
  };
}

interface Fetch {
  type: ActionType.Fetch;
  pubkey: PublicKey;
}

type Action = Update | Fetch;
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
            id: account.id,
            pubkey: account.pubkey,
            status: FetchStatus.Fetching
          }
        };
        return { ...state, accounts };
      } else {
        const idCounter = state.idCounter + 1;
        const accounts = {
          ...state.accounts,
          [address]: {
            id: idCounter,
            status: FetchStatus.Fetching,
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
        <HistoryProvider>{children}</HistoryProvider>
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
        data
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
