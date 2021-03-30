import React from "react";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import {
  ConfirmedSignatureInfo,
  Connection,
  ParsedConfirmedTransaction,
  PublicKey,
} from "@solana/web3.js";
import { useCluster } from "providers/cluster";

type DetailedAccountHistory = {
  transactionMap: Map<string, ParsedConfirmedTransaction>;
};

type DetailedAccountHistoryUpdate = {
  transactionMap: Map<string, ParsedConfirmedTransaction>;
};

type State = Cache.State<DetailedAccountHistory>;
type Dispatch = Cache.Dispatch<DetailedAccountHistoryUpdate>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type DetailedHistoryProviderProps = { children: React.ReactNode };
export function DetailedHistoryProvider({
  children,
}: DetailedHistoryProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useCustomReducer(url, reconcile);

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

function reconcile(
  history: DetailedAccountHistory | undefined,
  update: DetailedAccountHistoryUpdate | undefined
) {
  if (!history) {
    return update;
  }

  if (!update) {
    return history;
  }

  return {
    transactionMap: new Map([
      ...history.transactionMap,
      ...update.transactionMap,
    ]),
  };
}

export function useDetailedAccountHistory(
  address: string
): Map<string, ParsedConfirmedTransaction> {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useAccountHistory must be used within a AccountsProvider`);
  }

  return (
    context.entries[address]?.data?.transactionMap ||
    new Map<string, ParsedConfirmedTransaction>()
  );
}

async function fetchDetailedAccountHistory(
  dispatch: Dispatch,
  pubkey: PublicKey,
  signatures: string[],
  url: string
) {
  dispatch({
    type: ActionType.Update,
    status: FetchStatus.Fetching,
    key: pubkey.toBase58(),
    url,
  });

  try {
    const connection = new Connection(url);
    // @ts-ignore
    const fetched = await connection.getParsedConfirmedTransactions(signatures);

    const transactionMap = new Map();
    fetched.forEach((parsed: ParsedConfirmedTransaction | null, index: number) => {
      if (parsed !== null) {
        transactionMap.set(signatures[index], parsed)
      }
    });

    dispatch({
      type: ActionType.Update,
      url,
      key: pubkey.toBase58(),
      status: FetchStatus.Fetched,
      data: {
        transactionMap,
      },
    });
  } catch (error) {
    dispatch({
      type: ActionType.Update,
      url,
      key: pubkey.toBase58(),
      status: FetchStatus.FetchFailed,
    });
  }
}

export function useFetchDetailedAccountHistory(pubkey: PublicKey) {
  const { url } = useCluster();
  const state = React.useContext(StateContext);
  const dispatch = React.useContext(DispatchContext);

  if (!state || !dispatch) {
    throw new Error(
      `useFetchAccountHistory must be used within a AccountsProvider`
    );
  }

  return React.useCallback(
    (history: ConfirmedSignatureInfo[]) => {
      const existingMap =
        state.entries[pubkey.toBase58()]?.data?.transactionMap || new Map();
      const allSignatures = history.map(
        (signatureInfo) => signatureInfo.signature
      );
      const signaturesToFetch = allSignatures.filter(
        (signature) => !existingMap.has(signature)
      );

      if (signaturesToFetch.length < 1) {
        return;
      }

      fetchDetailedAccountHistory(dispatch, pubkey, signaturesToFetch, url);
    },
    [state, dispatch, url, pubkey]
  );
}
