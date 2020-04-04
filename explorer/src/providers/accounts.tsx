import React from "react";
import { PublicKey, Connection } from "@solana/web3.js";
import { findGetParameter, findPathSegment } from "../utils";
import { useCluster, ClusterStatus } from "./cluster";

export enum Status {
  Checking,
  CheckFailed,
  Success
}

enum Source {
  Url,
  Input
}

export interface Details {
  executable: boolean;
  owner: PublicKey;
  lamports: number;
  space: number;
}

export interface Account {
  id: number;
  status: Status;
  source: Source;
  pubkey: PublicKey;
  details?: Details;
}

type Accounts = { [address: string]: Account };
interface State {
  idCounter: number;
  accounts: Accounts;
}

export enum ActionType {
  Update,
  Input
}

interface Update {
  type: ActionType.Update;
  address: string;
  status: Status;
  details?: Details;
}

interface Input {
  type: ActionType.Input;
  pubkey: PublicKey;
}

type Action = Update | Input;
type Dispatch = (action: Action) => void;

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
          source: Source.Input,
          pubkey: action.pubkey
        }
      };
      return { ...state, accounts, idCounter };
    }
    case ActionType.Update: {
      let account = state.accounts[action.address];
      if (account) {
        account = {
          ...account,
          status: action.status,
          details: action.details
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

function urlAddresses(): Array<string> {
  const addresses: Array<string> = [];
  return addresses
    .concat(findGetParameter("account")?.split(",") || [])
    .concat(findGetParameter("accounts")?.split(",") || [])
    .concat(findPathSegment("account")?.split(",") || [])
    .concat(findPathSegment("accounts")?.split(",") || [])
    .concat(findGetParameter("address")?.split(",") || [])
    .concat(findGetParameter("addresses")?.split(",") || [])
    .concat(findPathSegment("address")?.split(",") || [])
    .concat(findPathSegment("addresses")?.split(",") || [])
    .filter(a => a.length > 0);
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
        source: Source.Url,
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
    status: Status.Checking,
    address
  });

  let status;
  let details;
  try {
    const result = await new Connection(url).getAccountInfo(
      new PublicKey(address)
    );
    details = {
      space: result.data.length,
      executable: result.executable,
      lamports: result.lamports,
      owner: result.owner
    };
    status = Status.Success;
  } catch (error) {
    console.error("Failed to fetch account info", error);
    status = Status.CheckFailed;
  }
  dispatch({ type: ActionType.Update, status, details, address });
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

export function useAccountsDispatch() {
  const context = React.useContext(DispatchContext);
  if (!context) {
    throw new Error(
      `useAccountsDispatch must be used within a AccountsProvider`
    );
  }
  return context;
}
