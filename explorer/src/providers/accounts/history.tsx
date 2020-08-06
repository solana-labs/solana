import React from "react";
import { PublicKey } from "@solana/web3.js";
import { useAccounts, FetchStatus } from "./index";
import { useCluster } from "../cluster";
import {
  HistoryManager,
  HistoricalTransaction,
  SlotRange,
} from "./historyManager";

interface AccountHistory {
  status: FetchStatus;
  fetched?: HistoricalTransaction[];
  fetchedRange?: SlotRange;
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
  fetched?: HistoricalTransaction[];
  fetchedRange?: SlotRange;
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

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.Add: {
      const details = { ...state };
      const address = action.address;
      if (!details[address]) {
        details[address] = {
          status: FetchStatus.Fetching,
        };
      }
      return details;
    }

    case ActionType.Update: {
      const address = action.pubkey.toBase58();
      if (state[address]) {
        const fetched = action.fetched
          ? action.fetched
          : state[address].fetched;
        const fetchedRange = action.fetchedRange
          ? action.fetchedRange
          : state[address].fetchedRange;
        return {
          ...state,
          [address]: {
            status: action.status,
            fetched,
            fetchedRange,
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

const ManagerContext = React.createContext<HistoryManager | undefined>(
  undefined
);
const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type HistoryProviderProps = { children: React.ReactNode };
export function HistoryProvider({ children }: HistoryProviderProps) {
  const [state, dispatch] = React.useReducer(reducer, {});
  const { accounts, lastFetchedAddress } = useAccounts();
  const { url } = useCluster();

  const manager = React.useRef(new HistoryManager(url));
  React.useEffect(() => {
    manager.current = new HistoryManager(url);
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
        fetchAccountHistory(
          dispatch,
          new PublicKey(lastFetchedAddress),
          manager.current,
          true
        );
      }
    }
  }, [accounts, lastFetchedAddress]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <ManagerContext.Provider value={manager.current}>
      <StateContext.Provider value={state}>
        <DispatchContext.Provider value={dispatch}>
          {children}
        </DispatchContext.Provider>
      </StateContext.Provider>
    </ManagerContext.Provider>
  );
}

async function fetchAccountHistory(
  dispatch: Dispatch,
  pubkey: PublicKey,
  manager: HistoryManager,
  refresh?: boolean
) {
  dispatch({
    type: ActionType.Update,
    status: FetchStatus.Fetching,
    pubkey,
  });

  let status;
  let fetched;
  let fetchedRange;
  try {
    await manager.fetchAccountHistory(pubkey, refresh || false);
    fetched = manager.accountHistory.get(pubkey.toBase58()) || undefined;
    fetchedRange = manager.accountRanges.get(pubkey.toBase58()) || undefined;
    status = FetchStatus.Fetched;
  } catch (error) {
    console.error("Failed to fetch account history", error);
    status = FetchStatus.FetchFailed;
  }
  dispatch({ type: ActionType.Update, status, fetched, fetchedRange, pubkey });
}

export function useAccountHistory(address: string) {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useAccountHistory must be used within a AccountsProvider`);
  }

  return context[address];
}

export function useFetchAccountHistory() {
  const manager = React.useContext(ManagerContext);
  const dispatch = React.useContext(DispatchContext);
  if (!manager || !dispatch) {
    throw new Error(
      `useFetchAccountHistory must be used within a AccountsProvider`
    );
  }

  return (pubkey: PublicKey, refresh?: boolean) => {
    fetchAccountHistory(dispatch, pubkey, manager, refresh);
  };
}
