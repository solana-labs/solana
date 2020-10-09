import React from "react";
import {
  Connection,
  TransactionSignature,
  ParsedConfirmedTransaction,
  PublicKey,
} from "@solana/web3.js";
import { RetrieveSignature } from "@theronin/solarweave";
import { useCluster, Cluster } from "../cluster";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import { reportError } from "utils/sentry";

export interface Details {
  transaction?: ParsedConfirmedTransaction | null;
}

type State = Cache.State<Details>;
type Dispatch = Cache.Dispatch<Details>;

export const StateContext = React.createContext<State | undefined>(undefined);
export const DispatchContext = React.createContext<Dispatch | undefined>(
  undefined
);

type DetailsProviderProps = { children: React.ReactNode };
export function DetailsProvider({ children }: DetailsProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useReducer<Details>(url);

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

function ParseAsTransaction(tx: any, signature: string) {
  let slot = -1;

  let meta = {
    err: null,
    fee: -1,
    postBalances: [],
    preBalances: [],
  };

  let transaction = {
    message: {
      accountKeys: [],
      instructions: [],
      recentBlockhash: "",
    },
    signatures: [],
  };

  tx.Tags.forEach((tag: any) => {
    if (tag.name === "slot") {
      slot = Number(tag.value);
    }
  });

  tx.BlockData.transactions.forEach((t: any) => {
    if (t.transaction.signatures.indexOf(signature) !== -1) {
      transaction.message.accountKeys = t.transaction.message.accountKeys.map(
        (key: string) => {
          return { pubkey: new PublicKey(key), signer: false, writable: false };
        }
      );

      transaction.message.instructions = t.transaction.message.instructions.map(
        (i: any) => {
          const accounts = i.accounts.map((a: number) => {
            return transaction.message.accountKeys[a]["pubkey"];
          });
          const data = i.data;
          const programId =
            transaction.message.accountKeys[i.programIdIndex]["pubkey"];

          return { accounts, data, programId };
        }
      );

      transaction.message.recentBlockhash =
        t.transaction.message.recentBlockhash;

      transaction.signatures = t.transaction.signatures;

      meta = t.meta;
    }
  });

  return { slot, meta, transaction };
}

async function fetchDetails(
  dispatch: Dispatch,
  signature: TransactionSignature,
  cluster: Cluster,
  url: string,
  solarweave: string
) {
  dispatch({
    type: ActionType.Update,
    status: FetchStatus.Fetching,
    key: signature,
    url,
  });

  let fetchStatus;
  let transaction;
  try {
    const tx = await RetrieveSignature(signature, `${solarweave}-index`);

    if (tx) {
      transaction = ParseAsTransaction(tx, signature);
    } else {
      transaction = await new Connection(url).getParsedConfirmedTransaction(
        signature
      );
    }

    fetchStatus = FetchStatus.Fetched;
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
    fetchStatus = FetchStatus.FetchFailed;
  }
  dispatch({
    type: ActionType.Update,
    status: fetchStatus,
    key: signature,
    data: { transaction },
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

  const { cluster, url, solarweave } = useCluster();
  return React.useCallback(
    (signature: TransactionSignature) => {
      url && fetchDetails(dispatch, signature, cluster, url, solarweave);
    },
    [dispatch, cluster, url, solarweave]
  );
}

export function useTransactionDetails(
  signature: TransactionSignature
): Cache.CacheEntry<Details> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useTransactionDetails must be used within a TransactionsProvider`
    );
  }

  return context.entries[signature];
}

export type TransactionDetailsCache = {
  [key: string]: Cache.CacheEntry<Details>;
};
export function useTransactionDetailsCache(): TransactionDetailsCache {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(
      `useTransactionDetailsCache must be used within a TransactionsProvider`
    );
  }

  return context.entries;
}
