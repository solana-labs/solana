import React from "react";
import { PublicKey, Connection } from "@solana/web3.js";
import { findGetParameter, findPathSegment } from "../utils";
import { useCluster } from "./cluster";

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

type Accounts = { [id: number]: Account };
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
  id: number;
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
      const idCounter = state.idCounter + 1;
      const accounts = {
        ...state.accounts,
        [idCounter]: {
          id: idCounter,
          status: Status.Checking,
          source: Source.Input,
          pubkey: action.pubkey
        }
      };
      return { ...state, accounts, idCounter };
    }
    case ActionType.Update: {
      let account = state.accounts[action.id];
      if (account) {
        account = {
          ...account,
          status: action.status,
          details: action.details
        };
        const accounts = {
          ...state.accounts,
          [action.id]: account
        };
        return { ...state, accounts };
      }
      break;
    }
  }
  return state;
}

function urlPublicKeys(): Array<PublicKey> {
  const keys: Array<string> = [];
  return keys
    .concat(findGetParameter("account")?.split(",") || [])
    .concat(findGetParameter("accounts")?.split(",") || [])
    .concat(findPathSegment("account")?.split(",") || [])
    .concat(findPathSegment("accounts")?.split(",") || [])
    .concat(findGetParameter("address")?.split(",") || [])
    .concat(findGetParameter("addresses")?.split(",") || [])
    .concat(findPathSegment("address")?.split(",") || [])
    .concat(findPathSegment("addresses")?.split(",") || [])
    .map(key => new PublicKey(key));
}

function initState(): State {
  let idCounter = 0;
  const pubkeys = urlPublicKeys();
  const accounts = pubkeys.reduce((accounts: Accounts, pubkey) => {
    const id = ++idCounter;
    accounts[id] = {
      id,
      status: Status.Checking,
      source: Source.Url,
      pubkey
    };
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
    Object.values(state.accounts).forEach(account => {
      fetchAccountInfo(dispatch, account.id, account.pubkey, url);
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
  id: number,
  pubkey: PublicKey,
  url: string
) {
  dispatch({
    type: ActionType.Update,
    status: Status.Checking,
    id
  });

  let status;
  let details;
  try {
    const result = await new Connection(url).getAccountInfo(pubkey);
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
  dispatch({ type: ActionType.Update, status, details, id });
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
