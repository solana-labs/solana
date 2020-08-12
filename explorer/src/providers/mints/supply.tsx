import React from "react";
import { useCluster } from "providers/cluster";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import { TokenAmount, PublicKey, Connection } from "@solana/web3.js";

type State = Cache.State<TokenAmount>;
type Dispatch = Cache.Dispatch<TokenAmount>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type ProviderProps = { children: React.ReactNode };
export function SupplyProvider({ children }: ProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useReducer<TokenAmount>(url);

  // Clear cache whenever cluster is changed
  React.useEffect(() => {
    dispatch({ type: ActionType.Clear, url });
  }, [dispatch, url]);

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        {children}
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

async function fetchSupply(dispatch: Dispatch, pubkey: PublicKey, url: string) {
  dispatch({
    type: ActionType.Update,
    key: pubkey.toBase58(),
    status: Cache.FetchStatus.Fetching,
    url,
  });

  let data;
  let fetchStatus;
  try {
    data = (await new Connection(url, "single").getTokenSupply(pubkey)).value;

    fetchStatus = FetchStatus.Fetched;
  } catch (error) {
    fetchStatus = FetchStatus.FetchFailed;
  }
  dispatch({
    type: ActionType.Update,
    status: fetchStatus,
    data,
    key: pubkey.toBase58(),
    url,
  });
}

export function useFetchTokenSupply() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(`useFetchTokenSupply must be used within a MintsProvider`);
  }

  const { url } = useCluster();
  return (pubkey: PublicKey) => {
    fetchSupply(dispatch, pubkey, url);
  };
}

export function useTokenSupply(
  address: string
): Cache.CacheEntry<TokenAmount> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useTokenSupply must be used within a MintsProvider`);
  }

  return context.entries[address];
}
