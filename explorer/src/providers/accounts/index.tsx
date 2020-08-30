import React from "react";
import * as Sentry from "@sentry/react";
import { StakeAccount as StakeAccountWasm } from "solana-sdk-wasm";
import { PublicKey, Connection, StakeProgram } from "@solana/web3.js";
import { useCluster, Cluster } from "../cluster";
import { HistoryProvider } from "./history";
import { TokensProvider, TOKEN_PROGRAM_ID } from "./tokens";
import { coerce } from "superstruct";
import { ParsedInfo } from "validators";
import { StakeAccount } from "validators/accounts/stake";
import {
  TokenAccount,
  MintAccountInfo,
  TokenAccountInfo,
} from "validators/accounts/token";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
export { useAccountHistory } from "./history";

export type StakeProgramData = {
  program: "stake";
  parsed: StakeAccount | StakeAccountWasm;
};

export type TokenProgramData = {
  program: "spl-token";
  parsed: TokenAccount;
};

export type ProgramData = StakeProgramData | TokenProgramData;

export interface Details {
  executable: boolean;
  owner: PublicKey;
  space?: number;
  data?: ProgramData;
}

export interface Account {
  pubkey: PublicKey;
  lamports: number;
  details?: Details;
}

type State = Cache.State<Account>;
type Dispatch = Cache.Dispatch<Account>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type AccountsProviderProps = { children: React.ReactNode };
export function AccountsProvider({ children }: AccountsProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useReducer<Account>(url);

  // Clear accounts cache whenever cluster is changed
  React.useEffect(() => {
    dispatch({ type: ActionType.Clear, url });
  }, [dispatch, url]);

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        <TokensProvider>
          <HistoryProvider>{children}</HistoryProvider>
        </TokensProvider>
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

async function fetchAccountInfo(
  dispatch: Dispatch,
  pubkey: PublicKey,
  cluster: Cluster,
  url: string
) {
  dispatch({
    type: ActionType.Update,
    key: pubkey.toBase58(),
    status: Cache.FetchStatus.Fetching,
    url,
  });

  let data;
  let fetchStatus;
  try {
    const result = (
      await new Connection(url, "single").getParsedAccountInfo(pubkey)
    ).value;

    let lamports, details;
    if (result === null) {
      lamports = 0;
    } else {
      lamports = result.lamports;

      // Only save data in memory if we can decode it
      let space;
      if (!("parsed" in result.data)) {
        space = result.data.length;
      }

      let data: ProgramData | undefined;
      if (result.owner.equals(StakeProgram.programId)) {
        try {
          let parsed;
          if ("parsed" in result.data) {
            const info = coerce(result.data.parsed, ParsedInfo);
            parsed = coerce(info, StakeAccount);
          } else {
            const wasm = await import("solana-sdk-wasm");
            parsed = wasm.StakeAccount.fromAccountData(result.data);
          }
          data = {
            program: "stake",
            parsed,
          };
        } catch (err) {
          Sentry.captureException(err, {
            tags: { url, address: pubkey.toBase58() },
          });
          // TODO store error state in Account info
        }
      } else if ("parsed" in result.data) {
        if (result.owner.equals(TOKEN_PROGRAM_ID)) {
          try {
            const info = coerce(result.data.parsed, ParsedInfo);
            const parsed = coerce(info, TokenAccount);
            data = {
              program: "spl-token",
              parsed,
            };
          } catch (err) {
            Sentry.captureException(err, {
              tags: { url, address: pubkey.toBase58() },
            });
            // TODO store error state in Account info
          }
        }
      }

      details = {
        space,
        executable: result.executable,
        owner: result.owner,
        data,
      };
    }
    data = { pubkey, lamports, details };
    fetchStatus = FetchStatus.Fetched;
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      Sentry.captureException(error, { tags: { url } });
    }
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

export function useAccounts() {
  const context = React.useContext(StateContext);
  if (!context) {
    throw new Error(`useAccounts must be used within a AccountsProvider`);
  }
  return context.entries;
}

export function useAccountInfo(
  address: string | undefined
): Cache.CacheEntry<Account> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useAccountInfo must be used within a AccountsProvider`);
  }
  if (address === undefined) return;
  return context.entries[address];
}

export function useMintAccountInfo(
  address: string | undefined
): MintAccountInfo | undefined {
  const accountInfo = useAccountInfo(address);
  if (address === undefined) return;

  try {
    const data = accountInfo?.data?.details?.data;
    if (!data) return;
    if (data.program !== "spl-token" || data.parsed.type !== "mint") {
      throw new Error("Expected mint");
    }

    return coerce(data.parsed.info, MintAccountInfo);
  } catch (err) {
    Sentry.captureException(err, {
      tags: { address },
    });
  }
}

export function useTokenAccountInfo(
  address: string | undefined
): TokenAccountInfo | undefined {
  const accountInfo = useAccountInfo(address);
  if (address === undefined) return;

  try {
    const data = accountInfo?.data?.details?.data;
    if (!data) return;
    if (data.program !== "spl-token" || data.parsed.type !== "account") {
      throw new Error("Expected token account");
    }

    return coerce(data.parsed.info, TokenAccountInfo);
  } catch (err) {
    Sentry.captureException(err, {
      tags: { address },
    });
  }
}

export function useFetchAccountInfo() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(
      `useFetchAccountInfo must be used within a AccountsProvider`
    );
  }

  const { cluster, url } = useCluster();
  return React.useCallback(
    (pubkey: PublicKey) => {
      fetchAccountInfo(dispatch, pubkey, cluster, url);
    },
    [dispatch, cluster, url]
  );
}
