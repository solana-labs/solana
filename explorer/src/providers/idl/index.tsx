import React from "react";
import { Connection, PublicKey } from "@solana/web3.js";
import { useCluster, Cluster } from "../cluster";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import {
  decodeIdlAccount,
  idlAddress,
  Idl,
} from "@project-serum/anchor/dist/cjs/idl";
import { inflate } from "pako";
import { utf8 } from "@project-serum/anchor/dist/cjs/utils/bytes";
import { reportError } from "utils/sentry";

type State = Cache.State<Idl>;
type Dispatch = Cache.Dispatch<Idl>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type IdlProviderProps = { children: React.ReactNode };
export function IdlProvider({ children }: IdlProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useReducer<Idl>(url);

  // Clear idl cache whenever cluster is changed
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

export async function fetchIdl(
  programAddress: PublicKey,
  cluster: Cluster,
  url: string
): Promise<Idl | undefined> {
  const connection = new Connection(url, "confirmed");

  let idl;
  try {
    const idlAddr = await idlAddress(programAddress);
    const idlAccountInfo = await connection.getAccountInfo(idlAddr);

    if (idlAccountInfo) {
      // Chop off account discriminator.
      let idlAccount = decodeIdlAccount(idlAccountInfo.data.slice(8));
      const inflatedIdl = inflate(idlAccount.data);
      idl = JSON.parse(utf8.decode(inflatedIdl)) as Idl;
    }
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
  }
  return idl;
}

export async function fetchIdls(
  programAddresses: PublicKey[],
  url: string
): Promise<{ programAddress: PublicKey; idl: Idl | undefined }[]> {
  const connection = new Connection(url, "confirmed");

  const idlAddresses = await Promise.all(
    programAddresses.map(async (a) => await idlAddress(a))
  );
  const idlAccountInfos = await connection.getMultipleAccountsInfo(
    idlAddresses
  );

  const idls = idlAccountInfos.map((idlAccountInfo, i) => {
    let idl;
    if (idlAccountInfo) {
      // Chop off account discriminator.
      let idlAccount = decodeIdlAccount(idlAccountInfo.data.slice(8));
      const inflatedIdl = inflate(idlAccount.data);
      idl = JSON.parse(utf8.decode(inflatedIdl)) as Idl;
    }
    return { programAddress: programAddresses[i], idl };
  });

  return idls;
}

export async function fetchAndCacheIdl(
  dispatch: Dispatch,
  programAddress: PublicKey,
  cluster: Cluster,
  url: string
) {
  dispatch({
    type: ActionType.Update,
    key: programAddress.toBase58(),
    status: Cache.FetchStatus.Fetching,
    url,
  });

  let data;
  let fetchStatus;
  try {
    const idl = await fetchIdl(programAddress, cluster, url);
    fetchStatus = FetchStatus.Fetched;
    data = idl;
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
    fetchStatus = FetchStatus.FetchFailed;
  }
  dispatch({
    type: ActionType.Update,
    status: fetchStatus,
    data: data,
    key: programAddress.toBase58(),
    url,
  });
}

export async function fetchAndCacheIdls(
  dispatch: Dispatch,
  programAddresses: PublicKey[],
  cluster: Cluster,
  url: string
) {
  for (const address of programAddresses) {
    dispatch({
      type: ActionType.Update,
      key: address.toBase58(),
      status: Cache.FetchStatus.Fetching,
      url,
    });
  }

  let fetchStatus;
  try {
    const idls = await fetchIdls(programAddresses, url);
    fetchStatus = FetchStatus.Fetched;
    for (const idlInfo of idls) {
      dispatch({
        type: ActionType.Update,
        status: fetchStatus,
        data: idlInfo.idl,
        key: idlInfo.programAddress.toBase58(),
        url,
      });
    }
    return;
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
    fetchStatus = FetchStatus.FetchFailed;
  }
  for (const address of programAddresses) {
    dispatch({
      type: ActionType.Update,
      status: fetchStatus,
      data: undefined,
      key: address.toBase58(),
      url,
    });
  }
}

export function useIdl(
  programAddress: string | undefined
): Cache.CacheEntry<Idl> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useIdl must be used within a IdlProvider`);
  }
  if (programAddress === undefined) return;
  return context.entries[programAddress];
}

export function useIdls(
  programAddresses: string[] | undefined
): (Idl | undefined)[] | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useIdls must be used within a IdlProvider`);
  }

  if (programAddresses === undefined) return;
  return programAddresses.map((a) => context.entries[a]?.data);
}

export function useFetchIdl() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(`useFetchIdl must be used within a IdlProvider`);
  }

  const { cluster, url } = useCluster();
  return React.useCallback(
    (programAccount: PublicKey) => {
      fetchAndCacheIdl(dispatch, programAccount, cluster, url);
    },
    [dispatch, cluster, url]
  );
}

export function useFetchIdls() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(`useFetchIdl must be used within a IdlProvider`);
  }

  const { cluster, url } = useCluster();
  return React.useCallback(
    (programAddresses: PublicKey[]) => {
      fetchAndCacheIdls(dispatch, programAddresses, cluster, url);
    },
    [dispatch, cluster, url]
  );
}
