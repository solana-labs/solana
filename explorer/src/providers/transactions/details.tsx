import React from "react";
import {
  Connection,
  TransactionSignature,
  ParsedConfirmedTransaction,
} from "@solana/web3.js";
import { useCluster } from "../cluster";
import { FetchStatus } from "./index";
import { CACHED_DETAILS, isCached } from "./cached";

export interface Details {
  fetchStatus: FetchStatus;
  transaction: ParsedConfirmedTransaction | null;
}

type State = {
  entries: { [signature: string]: Details };
  url: string;
};

export enum ActionType {
  Update,
  Clear,
}

interface Update {
  type: ActionType.Update;
  url: string;
  signature: string;
  fetchStatus: FetchStatus;
  transaction: ParsedConfirmedTransaction | null;
}

interface Clear {
  type: ActionType.Clear;
  url: string;
}

type Action = Update | Clear;
type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  if (action.type === ActionType.Clear) {
    return { url: action.url, entries: {} };
  } else if (action.url !== state.url) {
    return state;
  }

  switch (action.type) {
    case ActionType.Update: {
      const signature = action.signature;
      const details = state.entries[signature];
      if (details) {
        return {
          ...state,
          entries: {
            ...state.entries,
            [signature]: {
              ...details,
              fetchStatus: action.fetchStatus,
              transaction: action.transaction,
            },
          },
        };
      } else {
        return {
          ...state,
          entries: {
            ...state.entries,
            [signature]: {
              fetchStatus: FetchStatus.Fetching,
              transaction: null,
            },
          },
        };
      }
    }
  }
}

export const StateContext = React.createContext<State | undefined>(undefined);
export const DispatchContext = React.createContext<Dispatch | undefined>(
  undefined
);

type DetailsProviderProps = { children: React.ReactNode };
export function DetailsProvider({ children }: DetailsProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = React.useReducer(reducer, { url, entries: {} });

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

async function fetchDetails(
  dispatch: Dispatch,
  signature: TransactionSignature,
  url: string
) {
  dispatch({
    type: ActionType.Update,
    fetchStatus: FetchStatus.Fetching,
    transaction: null,
    signature,
    url,
  });

  let fetchStatus;
  let transaction = null;
  if (isCached(url, signature)) {
    transaction = CACHED_DETAILS[signature];
    fetchStatus = FetchStatus.Fetched;
  } else {
    try {
      transaction = await new Connection(url).getParsedConfirmedTransaction(
        signature
      );
      fetchStatus = FetchStatus.Fetched;
    } catch (error) {
      console.error("Failed to fetch confirmed transaction", error);
      fetchStatus = FetchStatus.FetchFailed;
    }
  }
  dispatch({
    type: ActionType.Update,
    fetchStatus,
    signature,
    transaction,
    url,
  });
}

export function useFetchTransactionDetails() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchTransactionDetails must be used within a TransactionsProvider`
    );
  }

  const { url } = useCluster();
  return (signature: TransactionSignature) => {
    url && fetchDetails(dispatch, signature, url);
  };
}
