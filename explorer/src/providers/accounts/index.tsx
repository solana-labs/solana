import React from "react";
import { pubkeyToString } from "utils";
import { PublicKey, Connection, StakeActivationData } from "@solana/web3.js";
import { useCluster, Cluster } from "../cluster";
import { HistoryProvider } from "./history";
import { TokensProvider } from "./tokens";
import { create } from "superstruct";
import { ParsedInfo } from "validators";
import { StakeAccount } from "validators/accounts/stake";
import {
  TokenAccount,
  MintAccountInfo,
  TokenAccountInfo,
} from "validators/accounts/token";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import { reportError } from "utils/sentry";
import { VoteAccount } from "validators/accounts/vote";
import { NonceAccount } from "validators/accounts/nonce";
import { SysvarAccount } from "validators/accounts/sysvar";
import { ConfigAccount } from "validators/accounts/config";
import { FlaggedAccountsProvider } from "./flagged-accounts";
import {
  ProgramDataAccount,
  ProgramDataAccountInfo,
  UpgradeableLoaderAccount,
} from "validators/accounts/upgradeable-program";
import { RewardsProvider } from "./rewards";
import { programs, MetadataJson } from "@metaplex/js";
import getEditionInfo, { EditionInfo } from "./utils/getEditionInfo";
export { useAccountHistory } from "./history";

const Metadata = programs.metadata.Metadata;

export type StakeProgramData = {
  program: "stake";
  parsed: StakeAccount;
  activation?: StakeActivationData;
};

export type UpgradeableLoaderAccountData = {
  program: "bpf-upgradeable-loader";
  parsed: UpgradeableLoaderAccount;
  programData?: ProgramDataAccountInfo;
};

export type NFTData = {
  metadata: programs.metadata.MetadataData;
  json: MetadataJson | undefined;
  editionInfo: EditionInfo;
};

export type TokenProgramData = {
  program: "spl-token";
  parsed: TokenAccount;
  nftData?: NFTData;
};

export type VoteProgramData = {
  program: "vote";
  parsed: VoteAccount;
};

export type NonceProgramData = {
  program: "nonce";
  parsed: NonceAccount;
};

export type SysvarProgramData = {
  program: "sysvar";
  parsed: SysvarAccount;
};

export type ConfigProgramData = {
  program: "config";
  parsed: ConfigAccount;
};

export type ProgramData =
  | UpgradeableLoaderAccountData
  | StakeProgramData
  | TokenProgramData
  | VoteProgramData
  | NonceProgramData
  | SysvarProgramData
  | ConfigProgramData;

export interface Details {
  executable: boolean;
  owner: PublicKey;
  space: number;
  data?: ProgramData;
  rawData?: Buffer;
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
          <HistoryProvider>
            <RewardsProvider>
              <FlaggedAccountsProvider>{children}</FlaggedAccountsProvider>
            </RewardsProvider>
          </HistoryProvider>
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
    const connection = new Connection(url, "confirmed");
    const result = (await connection.getParsedAccountInfo(pubkey)).value;

    let lamports, details;
    if (result === null) {
      lamports = 0;
    } else {
      lamports = result.lamports;

      // Only save data in memory if we can decode it
      let space: number;
      if (!("parsed" in result.data)) {
        space = result.data.length;
      } else {
        space = result.data.space;
      }

      let data: ProgramData | undefined;
      if ("parsed" in result.data) {
        try {
          const info = create(result.data.parsed, ParsedInfo);
          switch (result.data.program) {
            case "bpf-upgradeable-loader": {
              const parsed = create(info, UpgradeableLoaderAccount);

              // Fetch program data to get program upgradeability info
              let programData: ProgramDataAccountInfo | undefined;
              if (parsed.type === "program") {
                const result = (
                  await connection.getParsedAccountInfo(parsed.info.programData)
                ).value;
                if (
                  result &&
                  "parsed" in result.data &&
                  result.data.program === "bpf-upgradeable-loader"
                ) {
                  const info = create(result.data.parsed, ParsedInfo);
                  programData = create(info, ProgramDataAccount).info;
                } else {
                  throw new Error(
                    `invalid program data account for program: ${pubkey.toBase58()}`
                  );
                }
              }

              data = {
                program: result.data.program,
                parsed,
                programData,
              };

              break;
            }
            case "stake": {
              const parsed = create(info, StakeAccount);
              const isDelegated = parsed.type === "delegated";
              const activation = isDelegated
                ? await connection.getStakeActivation(pubkey)
                : undefined;

              data = {
                program: result.data.program,
                parsed,
                activation,
              };
              break;
            }
            case "vote":
              data = {
                program: result.data.program,
                parsed: create(info, VoteAccount),
              };
              break;
            case "nonce":
              data = {
                program: result.data.program,
                parsed: create(info, NonceAccount),
              };
              break;
            case "sysvar":
              data = {
                program: result.data.program,
                parsed: create(info, SysvarAccount),
              };
              break;
            case "config":
              data = {
                program: result.data.program,
                parsed: create(info, ConfigAccount),
              };
              break;

            case "spl-token":
              const parsed = create(info, TokenAccount);
              let nftData;

              try {
                // Generate a PDA and check for a Metadata Account
                if (parsed.type === "mint") {
                  const metadata = await Metadata.load(
                    connection,
                    await Metadata.getPDA(pubkey)
                  );
                  if (metadata) {
                    // We have a valid Metadata account. Try and pull edition data.
                    const editionInfo = await getEditionInfo(
                      metadata,
                      connection
                    );
                    const id = pubkeyToString(pubkey);
                    const metadataJSON = await getMetaDataJSON(
                      id,
                      metadata.data
                    );
                    nftData = {
                      metadata: metadata.data,
                      json: metadataJSON,
                      editionInfo,
                    };
                  }
                }
              } catch (error) {
                // unable to find NFT metadata account
              }

              data = {
                program: result.data.program,
                parsed,
                nftData,
              };
              break;
            default:
              data = undefined;
          }
        } catch (error) {
          reportError(error, { url, address: pubkey.toBase58() });
        }
      }

      // If we cannot parse account layout as native spl account
      // then keep raw data for other components to decode
      let rawData: Buffer | undefined;
      if (!data && !("parsed" in result.data)) {
        rawData = result.data;
      }

      details = {
        space,
        executable: result.executable,
        owner: result.owner,
        data,
        rawData,
      };
    }
    data = { pubkey, lamports, details };
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
    data,
    key: pubkey.toBase58(),
    url,
  });
}

const IMAGE_MIME_TYPE_REGEX = /data:image\/(svg\+xml|png|jpeg|gif)/g;

const getMetaDataJSON = async (
  id: string,
  metadata: programs.metadata.MetadataData
): Promise<MetadataJson | undefined> => {
  return new Promise(async (resolve, reject) => {
    const uri = metadata.data.uri;
    if (!uri) return resolve(undefined);

    const processJson = (extended: any) => {
      if (!extended || extended?.properties?.files?.length === 0) {
        return;
      }

      if (extended?.image) {
        extended.image =
          extended.image.startsWith("http") ||
          IMAGE_MIME_TYPE_REGEX.test(extended.image)
            ? extended.image
            : `${metadata.data.uri}/${extended.image}`;
      }

      return extended;
    };

    try {
      fetch(uri)
        .then(async (_) => {
          try {
            const data = await _.json();
            try {
              localStorage.setItem(uri, JSON.stringify(data));
            } catch {
              // ignore
            }
            resolve(processJson(data));
          } catch {
            resolve(undefined);
          }
        })
        .catch(() => {
          resolve(undefined);
        });
    } catch (ex) {
      console.error(ex);
      resolve(undefined);
    }
  });
};

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
  return React.useMemo(() => {
    if (address === undefined) return;

    try {
      const data = accountInfo?.data?.details?.data;
      if (!data) return;
      if (data.program !== "spl-token" || data.parsed.type !== "mint") {
        return;
      }

      return create(data.parsed.info, MintAccountInfo);
    } catch (err) {
      reportError(err, { address });
    }
  }, [address, accountInfo]);
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
      return;
    }

    return create(data.parsed.info, TokenAccountInfo);
  } catch (err) {
    reportError(err, { address });
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
