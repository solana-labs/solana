import React from "react";
import { pubkeyToString } from "utils";
import {
  PublicKey,
  Connection,
  StakeActivationData,
  AddressLookupTableAccount,
  AddressLookupTableProgram,
  SystemProgram,
  ParsedAccountData,
} from "@solana/web3.js";
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
import { ParsedAddressLookupTableAccount } from "validators/accounts/address-lookup-table";
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

export type AddressLookupTableProgramData = {
  program: "address-lookup-table";
  parsed: ParsedAddressLookupTableAccount;
};

export type ParsedData =
  | UpgradeableLoaderAccountData
  | StakeProgramData
  | TokenProgramData
  | VoteProgramData
  | NonceProgramData
  | SysvarProgramData
  | ConfigProgramData
  | AddressLookupTableProgramData;

export interface AccountData {
  parsed?: ParsedData;
  raw?: Buffer;
}

export interface Account {
  pubkey: PublicKey;
  lamports: number;
  executable: boolean;
  owner: PublicKey;
  space?: number;
  data: AccountData;
}

type State = Cache.State<Account>;
type Dispatch = Cache.Dispatch<Account>;
type Fetchers = { [mode in FetchAccountDataMode]: MultipleAccountFetcher };

const FetchersContext = React.createContext<Fetchers | undefined>(undefined);
const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

class MultipleAccountFetcher {
  pubkeys: PublicKey[] = [];
  fetchTimeout?: NodeJS.Timeout;

  constructor(
    private dispatch: Dispatch,
    private cluster: Cluster,
    private url: string,
    private dataMode: FetchAccountDataMode
  ) {}
  fetch = (pubkey: PublicKey) => {
    if (this.pubkeys !== undefined) this.pubkeys.push(pubkey);
    if (this.fetchTimeout === undefined) {
      this.fetchTimeout = setTimeout(() => {
        this.fetchTimeout = undefined;
        if (this.pubkeys !== undefined) {
          const pubkeys = this.pubkeys;
          this.pubkeys = [];

          const { dispatch, cluster, url, dataMode } = this;
          fetchMultipleAccounts({ dispatch, pubkeys, cluster, url, dataMode });
        }
      }, 100);
    }
  };
}

export type FetchAccountDataMode = "parsed" | "raw" | "skip";

type AccountsProviderProps = { children: React.ReactNode };
export function AccountsProvider({ children }: AccountsProviderProps) {
  const { cluster, url } = useCluster();
  const [state, dispatch] = Cache.useReducer<Account>(url);
  const [fetchers, setFetchers] = React.useState<Fetchers>(() => ({
    skip: new MultipleAccountFetcher(dispatch, cluster, url, "skip"),
    raw: new MultipleAccountFetcher(dispatch, cluster, url, "raw"),
    parsed: new MultipleAccountFetcher(dispatch, cluster, url, "parsed"),
  }));

  // Clear accounts cache whenever cluster is changed
  React.useEffect(() => {
    dispatch({ type: ActionType.Clear, url });
    setFetchers({
      skip: new MultipleAccountFetcher(dispatch, cluster, url, "skip"),
      raw: new MultipleAccountFetcher(dispatch, cluster, url, "raw"),
      parsed: new MultipleAccountFetcher(dispatch, cluster, url, "parsed"),
    });
  }, [dispatch, cluster, url]);

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        <FetchersContext.Provider value={fetchers}>
          <TokensProvider>
            <HistoryProvider>
              <RewardsProvider>{children}</RewardsProvider>
            </HistoryProvider>
          </TokensProvider>
        </FetchersContext.Provider>
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

async function fetchMultipleAccounts({
  dispatch,
  pubkeys,
  dataMode,
  cluster,
  url,
}: {
  dispatch: Dispatch;
  pubkeys: PublicKey[];
  dataMode: FetchAccountDataMode;
  cluster: Cluster;
  url: string;
}) {
  for (let pubkey of pubkeys) {
    dispatch({
      type: ActionType.Update,
      key: pubkey.toBase58(),
      status: Cache.FetchStatus.Fetching,
      url,
    });
  }

  const BATCH_SIZE = 100;
  const connection = new Connection(url, "confirmed");

  let nextBatchStart = 0;
  while (nextBatchStart < pubkeys.length) {
    const batch = pubkeys.slice(nextBatchStart, nextBatchStart + BATCH_SIZE);
    nextBatchStart += BATCH_SIZE;

    try {
      let results;
      if (dataMode === "parsed") {
        results = (await connection.getMultipleParsedAccounts(batch)).value;
      } else if (dataMode === "raw") {
        results = await connection.getMultipleAccountsInfo(batch);
      } else {
        results = await connection.getMultipleAccountsInfo(batch, {
          dataSlice: { length: 0, offset: 0 },
        });
      }

      for (let i = 0; i < batch.length; i++) {
        const pubkey = batch[i];
        const result = results[i];

        let account: Account;
        if (result === null) {
          account = {
            pubkey,
            lamports: 0,
            owner: SystemProgram.programId,
            space: 0,
            executable: false,
            data: { raw: Buffer.alloc(0) },
          };
        } else {
          let space: number | undefined = undefined;
          let parsedData: ParsedData | undefined;
          if ("parsed" in result.data) {
            const accountData: ParsedAccountData = result.data;
            space = result.data.space;
            try {
              parsedData = await handleParsedAccountData(
                connection,
                pubkey,
                accountData
              );
            } catch (error) {
              reportError(error, { url, address: pubkey.toBase58() });
            }
          }

          // If we cannot parse account layout as native spl account
          // then keep raw data for other components to decode
          let rawData: Buffer | undefined;
          if (
            !parsedData &&
            !("parsed" in result.data) &&
            dataMode !== "skip"
          ) {
            space = result.data.length;
            rawData = result.data;
          }

          account = {
            pubkey,
            lamports: result.lamports,
            executable: result.executable,
            owner: result.owner,
            space,
            data: {
              parsed: parsedData,
              raw: rawData,
            },
          };
        }

        dispatch({
          type: ActionType.Update,
          status: FetchStatus.Fetched,
          data: account,
          key: pubkey.toBase58(),
          url,
        });
      }
    } catch (error) {
      if (cluster !== Cluster.Custom) {
        reportError(error, { url });
      }

      for (let pubkey of batch) {
        dispatch({
          type: ActionType.Update,
          status: FetchStatus.FetchFailed,
          key: pubkey.toBase58(),
          url,
        });
      }
    }
  }
}

async function handleParsedAccountData(
  connection: Connection,
  accountKey: PublicKey,
  accountData: ParsedAccountData
): Promise<ParsedData | undefined> {
  const info = create(accountData.parsed, ParsedInfo);
  switch (accountData.program) {
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
        }
      }

      return {
        program: accountData.program,
        parsed,
        programData,
      };
    }

    case "stake": {
      const parsed = create(info, StakeAccount);
      const isDelegated = parsed.type === "delegated";
      const activation = isDelegated
        ? await connection.getStakeActivation(accountKey)
        : undefined;

      return {
        program: accountData.program,
        parsed,
        activation,
      };
    }

    case "vote": {
      return {
        program: accountData.program,
        parsed: create(info, VoteAccount),
      };
    }

    case "nonce": {
      return {
        program: accountData.program,
        parsed: create(info, NonceAccount),
      };
    }

    case "sysvar": {
      return {
        program: accountData.program,
        parsed: create(info, SysvarAccount),
      };
    }

    case "config": {
      return {
        program: accountData.program,
        parsed: create(info, ConfigAccount),
      };
    }

    case "address-lookup-table": {
      const parsed = create(info, ParsedAddressLookupTableAccount);
      return {
        program: accountData.program,
        parsed,
      };
    }

    case "spl-token": {
      const parsed = create(info, TokenAccount);
      let nftData;

      try {
        // Generate a PDA and check for a Metadata Account
        if (parsed.type === "mint") {
          const metadata = await Metadata.load(
            connection,
            await Metadata.getPDA(accountKey)
          );
          if (metadata) {
            // We have a valid Metadata account. Try and pull edition data.
            const editionInfo = await getEditionInfo(metadata, connection);
            const id = pubkeyToString(accountKey);
            const metadataJSON = await getMetaDataJSON(id, metadata.data);
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

      return {
        program: accountData.program,
        parsed,
        nftData,
      };
    }
  }
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
    if (address === undefined || accountInfo?.data === undefined) return;
    const account = accountInfo.data;

    try {
      const parsedData = account.data.parsed;
      if (!parsedData) return;
      if (
        parsedData.program !== "spl-token" ||
        parsedData.parsed.type !== "mint"
      ) {
        return;
      }

      return create(parsedData.parsed.info, MintAccountInfo);
    } catch (err) {
      reportError(err, { address });
    }
  }, [address, accountInfo]);
}

export function useTokenAccountInfo(
  address: string | undefined
): TokenAccountInfo | undefined {
  const accountInfo = useAccountInfo(address);
  return React.useMemo(() => {
    if (address === undefined || accountInfo?.data === undefined) return;
    const account = accountInfo.data;

    try {
      const parsedData = account.data.parsed;
      if (!parsedData) return;
      if (
        parsedData.program !== "spl-token" ||
        parsedData.parsed.type !== "account"
      ) {
        return;
      }

      return create(parsedData.parsed.info, TokenAccountInfo);
    } catch (err) {
      reportError(err, { address });
    }
  }, [address, accountInfo]);
}

export function useAddressLookupTable(
  address: string
): [AddressLookupTableAccount | string | undefined, FetchStatus] | undefined {
  const accountInfo = useAccountInfo(address);
  return React.useMemo(() => {
    if (accountInfo === undefined) return;
    const account = accountInfo.data;
    if (account === undefined) return [account, accountInfo.status];
    if (account.lamports === 0)
      return ["Lookup Table Not Found", accountInfo.status];
    const { parsed: parsedData, raw: rawData } = account.data;

    const key = new PublicKey(address);
    if (parsedData && parsedData.program === "address-lookup-table") {
      if (parsedData.parsed.type === "lookupTable") {
        return [
          new AddressLookupTableAccount({
            key,
            state: parsedData.parsed.info,
          }),
          accountInfo.status,
        ];
      } else if (parsedData.parsed.type === "uninitialized") {
        return ["Lookup Table Uninitialized", accountInfo.status];
      }
    } else if (
      rawData &&
      account.owner.equals(AddressLookupTableProgram.programId)
    ) {
      try {
        return [
          new AddressLookupTableAccount({
            key,
            state: AddressLookupTableAccount.deserialize(rawData),
          }),
          accountInfo.status,
        ];
      } catch {}
    }

    return ["Invalid Lookup Table", accountInfo.status];
  }, [address, accountInfo]);
}

export function useFetchAccountInfo() {
  const fetchers = React.useContext(FetchersContext);
  if (!fetchers) {
    throw new Error(
      `useFetchAccountInfo must be used within a AccountsProvider`
    );
  }

  return React.useCallback(
    (pubkey: PublicKey, dataMode: FetchAccountDataMode) => {
      fetchers[dataMode].fetch(pubkey);
    },
    [fetchers]
  );
}
