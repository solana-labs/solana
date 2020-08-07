import React from "react";
import {
  PublicKey,
  ConfirmedSignatureInfo,
  TransactionSignature,
  Connection,
} from "@solana/web3.js";
import { useAccounts, FetchStatus } from "./index";
import { useCluster } from "../cluster";

interface AccountHistory {
  status: FetchStatus;
  fetched?: ConfirmedSignatureInfo[];
  foundOldest: boolean;
}

type State = { [address: string]: AccountHistory };

export enum ActionType {
  Update,
  Add,
  Clear,
}

interface Update {
  type: ActionType.Update;
  pubkey: PublicKey;
  status: FetchStatus;
  fetched?: ConfirmedSignatureInfo[];
  before?: TransactionSignature;
  foundOldest?: boolean;
}

interface Add {
  type: ActionType.Add;
  address: string;
}

interface Clear {
  type: ActionType.Clear;
}

type Action = Update | Add | Clear;
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
    case ActionType.Add: {
      const details = { ...state };
      const address = action.address;
      if (!details[address]) {
        details[address] = {
          status: FetchStatus.Fetching,
          foundOldest: false,
        };
      }
      return details;
    }

    case ActionType.Update: {
      const address = action.pubkey.toBase58();
      if (state[address]) {
        return {
          ...state,
          [address]: {
            status: action.status,
            fetched: combineFetched(
              action.fetched,
              state[address].fetched,
              action.before
            ),
            foundOldest: action.foundOldest || state[address].foundOldest,
          },
        };
      }
      break;
    }

    case ActionType.Clear: {
      return {};
    }
  }
  return state;
}

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type HistoryProviderProps = { children: React.ReactNode };
export function HistoryProvider({ children }: HistoryProviderProps) {
  const [state, dispatch] = React.useReducer(reducer, {});
  const { accounts, lastFetchedAddress } = useAccounts();
  const { url } = useCluster();

  React.useEffect(() => {
    dispatch({ type: ActionType.Clear });
  }, [url]);

  // Fetch history for new accounts
  React.useEffect(() => {
    if (lastFetchedAddress) {
      const infoFetched =
        accounts[lastFetchedAddress] &&
        accounts[lastFetchedAddress].lamports !== undefined;
      const noHistory = !state[lastFetchedAddress];
      if (infoFetched && noHistory) {
        dispatch({ type: ActionType.Add, address: lastFetchedAddress });
        fetchAccountHistory(dispatch, new PublicKey(lastFetchedAddress), url, {
          limit: 10,
        });
      }
    }
  }, [accounts, lastFetchedAddress]); // eslint-disable-line react-hooks/exhaustive-deps

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
    status,
    fetched,
    before: options?.before,
    pubkey,
    foundOldest,
  });
}

export function useAccountHistory(address: string) {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useAccountHistory must be used within a AccountsProvider`);
  }

  return context[address];
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
    const before = state[pubkey.toBase58()];
    if (!refresh && before && before.fetched && before.fetched.length > 0) {
      const oldest = before.fetched[before.fetched.length - 1].signature;
      fetchAccountHistory(dispatch, pubkey, url, { before: oldest, limit: 25 });
    } else {
      fetchAccountHistory(dispatch, pubkey, url, { limit: 25 });
    }
  };
}
