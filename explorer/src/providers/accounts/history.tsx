import React from "react";
import { PublicKey } from "@solana/web3.js";
import { useAccounts, FetchStatus } from "./index";
import { useCluster } from "../cluster";
import {
  HistoryManager,
  HistoricalTransaction,
  SlotRange
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
  Remove
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
  addresses: string[];
}

interface Remove {
  type: ActionType.Remove;
  addresses: string[];
}

type Action = Update | Add | Remove;
type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.Add: {
      if (action.addresses.length === 0) return state;
      const details = { ...state };
      action.addresses.forEach(address => {
        if (!details[address]) {
          details[address] = {
            status: FetchStatus.Fetching
          };
        }
      });
      return details;
    }

    case ActionType.Remove: {
      if (action.addresses.length === 0) return state;
      const details = { ...state };
      action.addresses.forEach(address => {
        delete details[address];
      });
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
            fetchedRange
          }
        };
      }
      break;
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
  const { accounts } = useAccounts();
  const { url } = useCluster();

  const manager = React.useRef(new HistoryManager(url));
  React.useEffect(() => {
    manager.current = new HistoryManager(url);
  }, [url]);

  // Fetch history for new accounts
  React.useEffect(() => {
    const removeAddresses = new Set<string>();
    const fetchAddresses = new Set<string>();
    accounts.forEach(({ pubkey, lamports }) => {
      const address = pubkey.toBase58();
      if (lamports !== undefined && !state[address])
        fetchAddresses.add(address);
      else if (lamports === undefined && state[address])
        removeAddresses.add(address);
    });

    const removeList: string[] = [];
    removeAddresses.forEach(address => {
      manager.current.removeAccountHistory(address);
      removeList.push(address);
    });
    dispatch({ type: ActionType.Remove, addresses: removeList });

    const fetchList: string[] = [];
    fetchAddresses.forEach(s => fetchList.push(s));
    dispatch({ type: ActionType.Add, addresses: fetchList });
    fetchAddresses.forEach(address => {
      fetchAccountHistory(
        dispatch,
        new PublicKey(address),
        manager.current,
        true
      );
    });
  }, [accounts]); // eslint-disable-line react-hooks/exhaustive-deps

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
    pubkey
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
