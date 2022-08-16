import React from "react";
import {
  PublicKey,
  ConfirmedSignatureInfo,
  TransactionSignature,
  Connection,
  ParsedTransactionWithMeta,
} from "@solana/web3.js";
import { useCluster, Cluster } from "../cluster";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import { reportError } from "utils/sentry";

const MAX_TRANSACTION_BATCH_SIZE = 10;

type TransactionMap = Map<string, ParsedTransactionWithMeta>;

type AccountHistory = {
  fetched: ConfirmedSignatureInfo[];
  transactionMap?: TransactionMap;
  foundOldest: boolean;
};

type HistoryUpdate = {
  history?: AccountHistory;
  transactionMap?: TransactionMap;
  before?: TransactionSignature;
};

type State = Cache.State<AccountHistory>;
type Dispatch = Cache.Dispatch<HistoryUpdate>;

function combineFetched(
  fetched: ConfirmedSignatureInfo[],
  current: ConfirmedSignatureInfo[] | undefined,
  before: TransactionSignature | undefined
) {
  if (current === undefined || current.length === 0) {
    return fetched;
  }

  // History was refreshed, fetch results should be prepended if contiguous
  if (before === undefined) {
    const end = fetched.findIndex((f) => f.signature === current[0].signature);
    if (end < 0) return fetched;
    return fetched.slice(0, end).concat(current);
  }

  // More history was loaded, fetch results should be appended
  if (current[current.length - 1].signature === before) {
    return current.concat(fetched);
  }

  return fetched;
}

function reconcile(
  history: AccountHistory | undefined,
  update: HistoryUpdate | undefined
) {
  if (update?.history === undefined) return history;

  let transactionMap = history?.transactionMap || new Map();
  if (update.transactionMap) {
    transactionMap = new Map([...transactionMap, ...update.transactionMap]);
  }

  return {
    fetched: combineFetched(
      update.history.fetched,
      history?.fetched,
      update?.before
    ),
    transactionMap,
    foundOldest: update?.history?.foundOldest || history?.foundOldest || false,
  };
}

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type HistoryProviderProps = { children: React.ReactNode };
export function HistoryProvider({ children }: HistoryProviderProps) {
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

async function fetchParsedTransactions(
  url: string,
  transactionSignatures: string[]
) {
  const transactionMap = new Map();
  const connection = new Connection(url);

  while (transactionSignatures.length > 0) {
    const signatures = transactionSignatures.splice(
      0,
      MAX_TRANSACTION_BATCH_SIZE
    );
    const fetched = await connection.getParsedTransactions(signatures);
    fetched.forEach(
      (
        transactionWithMeta: ParsedTransactionWithMeta | null,
        index: number
      ) => {
        if (transactionWithMeta !== null) {
          transactionMap.set(signatures[index], transactionWithMeta);
        }
      }
    );
  }

  return transactionMap;
}

async function fetchAccountHistory(
  dispatch: Dispatch,
  pubkey: PublicKey,
  cluster: Cluster,
  url: string,
  options: {
    before?: TransactionSignature;
    limit: number;
  },
  fetchTransactions?: boolean,
  additionalSignatures?: string[]
) {
  dispatch({
    type: ActionType.Update,
    status: FetchStatus.Fetching,
    key: pubkey.toBase58(),
    url,
  });

  let status;
  let history;
  try {
    const connection = new Connection(url);
    const fetched = await connection.getConfirmedSignaturesForAddress2(
      pubkey,
      options
    );
    history = {
      fetched,
      foundOldest: fetched.length < options.limit,
    };
    status = FetchStatus.Fetched;
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
    status = FetchStatus.FetchFailed;
  }

  let transactionMap;
  if (fetchTransactions && history?.fetched) {
    try {
      const signatures = history.fetched
        .map((signature) => signature.signature)
        .concat(additionalSignatures || []);
      transactionMap = await fetchParsedTransactions(url, signatures);
    } catch (error) {
      if (cluster !== Cluster.Custom) {
        reportError(error, { url });
      }
      status = FetchStatus.FetchFailed;
    }
  }

  dispatch({
    type: ActionType.Update,
    url,
    key: pubkey.toBase58(),
    status,
    data: {
      history,
      transactionMap,
      before: options?.before,
    },
  });
}

export function useAccountHistories() {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useAccountHistories must be used within a AccountsProvider`
    );
  }

  return context.entries;
}

export function useAccountHistory(
  address: string
): Cache.CacheEntry<AccountHistory> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useAccountHistory must be used within a AccountsProvider`);
  }

  return context.entries[address];
}

function getUnfetchedSignatures(before: Cache.CacheEntry<AccountHistory>) {
  if (!before.data?.transactionMap) {
    return [];
  }

  const existingMap = before.data.transactionMap;
  const allSignatures = before.data.fetched.map(
    (signatureInfo) => signatureInfo.signature
  );
  return allSignatures.filter((signature) => !existingMap.has(signature));
}

export function useFetchAccountHistory() {
  const { cluster, url } = useCluster();
  const state = React.useContext(StateContext);
  const dispatch = React.useContext(DispatchContext);
  if (!state || !dispatch) {
    throw new Error(
      `useFetchAccountHistory must be used within a AccountsProvider`
    );
  }

  return React.useCallback(
    (pubkey: PublicKey, fetchTransactions?: boolean, refresh?: boolean) => {
      const before = state.entries[pubkey.toBase58()];
      if (!refresh && before?.data?.fetched && before.data.fetched.length > 0) {
        if (before.data.foundOldest) return;

        let additionalSignatures: string[] = [];
        if (fetchTransactions) {
          additionalSignatures = getUnfetchedSignatures(before);
        }

        const oldest =
          before.data.fetched[before.data.fetched.length - 1].signature;
        fetchAccountHistory(
          dispatch,
          pubkey,
          cluster,
          url,
          {
            before: oldest,
            limit: 25,
          },
          fetchTransactions,
          additionalSignatures
        );
      } else {
        fetchAccountHistory(
          dispatch,
          pubkey,
          cluster,
          url,
          { limit: 25 },
          fetchTransactions
        );
      }
    },
    [state, dispatch, cluster, url]
  );
}
