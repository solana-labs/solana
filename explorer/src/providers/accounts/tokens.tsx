import React from "react";
import { Connection, PublicKey } from "@solana/web3.js";
import { FetchStatus } from "./index";
import { useCluster } from "../cluster";
import { number, string, boolean, coerce, object, nullable } from "superstruct";

export type TokenAccountData = {
  mint: PublicKey;
  owner: PublicKey;
  amount: number;
  isInitialized: boolean;
  isNative: boolean;
};

const TokenAccountInfo = object({
  mint: string(),
  owner: string(),
  amount: number(),
  delegate: nullable(string()),
  delegatedAmount: number(),
  isInitialized: boolean(),
  isNative: boolean(),
});

interface AccountTokens {
  status: FetchStatus;
  tokens?: TokenAccountData[];
}

interface Update {
  type: "update";
  url: string;
  pubkey: PublicKey;
  status: FetchStatus;
  tokens?: TokenAccountData[];
}

interface Clear {
  type: "clear";
  url: string;
}

type Action = Update | Clear;
type State = {
  url: string;
  map: { [address: string]: AccountTokens };
};

type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  if (action.type === "clear") {
    return {
      url: action.url,
      map: {},
    };
  } else if (action.url !== state.url) {
    return state;
  }

  const address = action.pubkey.toBase58();
  let addressEntry = state.map[address];
  if (addressEntry && action.status === FetchStatus.Fetching) {
    addressEntry = {
      ...addressEntry,
      status: FetchStatus.Fetching,
    };
  } else {
    addressEntry = {
      tokens: action.tokens,
      status: action.status,
    };
  }

  return {
    ...state,
    map: {
      ...state.map,
      [address]: addressEntry,
    },
  };
}

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type ProviderProps = { children: React.ReactNode };
export function TokensProvider({ children }: ProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = React.useReducer(reducer, { url, map: {} });

  React.useEffect(() => {
    dispatch({ url, type: "clear" });
  }, [url]);

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        {children}
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

const TOKEN_PROGRAM_ID = new PublicKey(
  "TokenSVp5gheXUvJ6jGWGeCsgPKgnE3YgdGKRVCMY9o"
);

async function fetchAccountTokens(
  dispatch: Dispatch,
  pubkey: PublicKey,
  url: string
) {
  dispatch({
    type: "update",
    status: FetchStatus.Fetching,
    pubkey,
    url,
  });

  let status;
  let tokens;
  try {
    const { value } = await new Connection(
      url,
      "recent"
    ).getParsedTokenAccountsByOwner(pubkey, { programId: TOKEN_PROGRAM_ID });
    tokens = value.map((accountInfo) => {
      const parsedInfo = accountInfo.account.data.parsed.info;
      const info = coerce(parsedInfo, TokenAccountInfo);
      return {
        mint: new PublicKey(info.mint),
        owner: new PublicKey(info.owner),
        amount: info.amount,
        isInitialized: info.isInitialized,
        isNative: info.isNative,
      };
    });
    status = FetchStatus.Fetched;
  } catch (error) {
    status = FetchStatus.FetchFailed;
  }
  dispatch({ type: "update", url, status, tokens, pubkey });
}

export function useAccountOwnedTokens(address: string) {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useAccountOwnedTokens must be used within a AccountsProvider`
    );
  }

  return context.map[address];
}

export function useFetchAccountOwnedTokens() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchAccountOwnedTokens must be used within a AccountsProvider`
    );
  }

  const { url } = useCluster();
  return (pubkey: PublicKey) => {
    fetchAccountTokens(dispatch, pubkey, url);
  };
}
