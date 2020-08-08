import React from "react";
import {
  PublicKey,
  ConfirmedSignatureInfo,
  TransactionSignature,
  Connection,
} from "@solana/web3.js";
import { FetchStatus } from "./index";
import { useCluster } from "../cluster";

interface AccountHistory {
  status: FetchStatus;
  fetched?: ConfirmedSignatureInfo[];
  foundOldest: boolean;
}

type State = {
  url: string;
  map: { [address: string]: AccountHistory };
};

export enum ActionType {
  Update,
  Clear,
}

interface Update {
  type: ActionType.Update;
  url: string;
  pubkey: PublicKey;
  status: FetchStatus;
  fetched?: ConfirmedSignatureInfo[];
  before?: TransactionSignature;
  foundOldest?: boolean;
}

interface Clear {
  type: ActionType.Clear;
  url: string;
}

type Action = Update | Clear;
type Dispatch = (action: Action) => void;

function combineFetched(
  fetched: ConfirmedSignatureInfo[] | undefined,
  current: ConfirmedSignatureInfo[] | undefined,
  before: TransactionSignature | undefined
) {
  if (fetched === undefined) {
    return current;
  } else if (current === undefined) {
    return fetched;
  }

  if (current.length > 0 && current[current.length - 1].signature === before) {
    return current.concat(fetched);
  } else {
    return fetched;
  }
}

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.Update: {
      if (action.url !== state.url) return state;
      const address = action.pubkey.toBase58();
      if (state.map[address]) {
        return {
          ...state,
          map: {
            ...state.map,
            [address]: {
              status: action.status,
              fetched: combineFetched(
                action.fetched,
                state.map[address].fetched,
                action.before
              ),
              foundOldest: action.foundOldest || state.map[address].foundOldest,
            },
          },
        };
      } else {
        return {
          ...state,
          map: {
            ...state.map,
            [address]: {
              status: action.status,
              fetched: action.fetched,
              foundOldest: action.foundOldest || false,
            },
          },
        };
      }
    }

    case ActionType.Clear: {
      return { url: action.url, map: {} };
    }
  }
}

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type HistoryProviderProps = { children: React.ReactNode };
export function HistoryProvider({ children }: HistoryProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = React.useReducer(reducer, { url, map: {} });

  React.useEffect(() => {
    dispatch({ type: ActionType.Clear, url });
  }, [url]);

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        {children}
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

async function fetchAccountHistory(
  dispatch: Dispatch,
  pubkey: PublicKey,
  url: string,
  options: { before?: TransactionSignature; limit: number }
) {
  dispatch({
    type: ActionType.Update,
    status: FetchStatus.Fetching,
    pubkey,
    url,
  });

  let status;
  let fetched;
  let foundOldest;
  try {
    const connection = new Connection(url);
    fetched = await connection.getConfirmedSignaturesForAddress2(
      pubkey,
      options
    );
    foundOldest = fetched.length < options.limit;
    status = FetchStatus.Fetched;
  } catch (error) {
    console.error("Failed to fetch account history", error);
    status = FetchStatus.FetchFailed;
  }
  dispatch({
    type: ActionType.Update,
    url,
    status,
    fetched,
    before: options?.before,
    pubkey,
    foundOldest,
  });
}

export function useAccountHistories() {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useAccountHistories must be used within a AccountsProvider`
    );
  }

  return context.map;
}

export function useAccountHistory(address: string) {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useAccountHistory must be used within a AccountsProvider`);
  }

  return context.map[address];
}

export function useFetchAccountHistory() {
  const { url } = useCluster();
  const state = React.useContext(StateContext);
  const dispatch = React.useContext(DispatchContext);
  if (!state || !dispatch) {
    throw new Error(
      `useFetchAccountHistory must be used within a AccountsProvider`
    );
  }

  return (pubkey: PublicKey, refresh?: boolean) => {
    const before = state.map[pubkey.toBase58()];
    if (!refresh && before && before.fetched && before.fetched.length > 0) {
      if (before.foundOldest) return;
      const oldest = before.fetched[before.fetched.length - 1].signature;
      fetchAccountHistory(dispatch, pubkey, url, { before: oldest, limit: 25 });
    } else {
      fetchAccountHistory(dispatch, pubkey, url, { limit: 25 });
    }
  };
}
