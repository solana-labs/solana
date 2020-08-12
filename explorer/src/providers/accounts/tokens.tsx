import React from "react";
import { Connection, PublicKey } from "@solana/web3.js";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import { TokenAccountInfo } from "validators/accounts/token";
import { useCluster } from "../cluster";
import { coerce } from "superstruct";

export type TokenInfoWithPubkey = {
  info: TokenAccountInfo;
  pubkey: PublicKey;
};

interface AccountTokens {
  tokens?: TokenInfoWithPubkey[];
}

type State = Cache.State<AccountTokens>;
type Dispatch = Cache.Dispatch<AccountTokens>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type ProviderProps = { children: React.ReactNode };
export function TokensProvider({ children }: ProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useReducer<AccountTokens>(url);

  React.useEffect(() => {
    dispatch({ url, type: ActionType.Clear });
  }, [dispatch, url]);

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        {children}
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

export const TOKEN_PROGRAM_ID = new PublicKey(
  "TokenSVp5gheXUvJ6jGWGeCsgPKgnE3YgdGKRVCMY9o"
);

async function fetchAccountTokens(
  dispatch: Dispatch,
  pubkey: PublicKey,
  url: string
) {
  const key = pubkey.toBase58();
  dispatch({
    type: ActionType.Update,
    key,
    status: FetchStatus.Fetching,
    url,
  });

  let status;
  let data;
  try {
    const { value } = await new Connection(
      url,
      "recent"
    ).getParsedTokenAccountsByOwner(pubkey, { programId: TOKEN_PROGRAM_ID });
    data = {
      tokens: value.map((accountInfo) => {
        const parsedInfo = accountInfo.account.data.parsed.info;
        const info = coerce(parsedInfo, TokenAccountInfo);
        return { info, pubkey: accountInfo.pubkey };
      }),
    };
    status = FetchStatus.Fetched;
  } catch (error) {
    status = FetchStatus.FetchFailed;
  }
  dispatch({ type: ActionType.Update, url, status, data, key });
}

export function useAccountOwnedTokens(
  address: string
): Cache.CacheEntry<AccountTokens> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useAccountOwnedTokens must be used within a AccountsProvider`
    );
  }

  return context.entries[address];
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
