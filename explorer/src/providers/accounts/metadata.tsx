/*
    Alot of logic is from: https://github.com/metaplex-foundation/metaplex/blob/master/js/packages/common/src/actions/metadata.ts
*/

import React from "react";
import { Connection, PublicKey } from "@solana/web3.js";
import * as Cache from "providers/cache";
import { ActionType, FetchStatus } from "providers/cache";
import { useCluster, Cluster } from "../cluster";
import { Metadata, METADATA_SCHEMA } from "metaplex/classes";
import { METADATA_PREFIX, StringPublicKey } from "metaplex/types";
import { deserializeUnchecked, BinaryReader, BinaryWriter } from 'borsh';
import base58 from "bs58";
import { TOKEN_PROGRAM_ID } from "./tokens";
import {
    METADATA_PROGRAM_ID,
    SPL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_ID,
    METAPLEX_ID,
    BPF_UPGRADE_LOADER_ID,
    SYSTEM,
    MEMO_ID,
    VAULT_ID,
    AUCTION_ID,
    toPublicKey,
} from "metaplex/ids";
import { findProgramAddress } from "metaplex/utils";
import { reportError } from "utils/sentry";

export interface AccountMetadata {
    meta?: Metadata;
}

type State = Cache.State<AccountMetadata>;
type Dispatch = Cache.Dispatch<AccountMetadata>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type ProviderProps = { children: React.ReactNode };
export function MetadataProvider({ children }: ProviderProps) {
    const { url } = useCluster();
    const [state, dispatch] = Cache.useReducer<AccountMetadata>(url);

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

let STORE: PublicKey | undefined;

export const programIds = () => {
    return {
        token: TOKEN_PROGRAM_ID,
        associatedToken: SPL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_ID,
        bpf_upgrade_loader: BPF_UPGRADE_LOADER_ID,
        system: SYSTEM,
        metadata: METADATA_PROGRAM_ID,
        memo: MEMO_ID,
        vault: VAULT_ID,
        auction: AUCTION_ID,
        metaplex: METAPLEX_ID,
        store: STORE,
    };
};

export async function getMetadata(
    tokenMint: StringPublicKey,
): Promise<StringPublicKey> {
    const PROGRAM_IDS = programIds();

    return (
        await findProgramAddress(
            [
                Buffer.from(METADATA_PREFIX),
                toPublicKey(PROGRAM_IDS.metadata).toBuffer(),
                toPublicKey(tokenMint).toBuffer(),
            ],
            toPublicKey(PROGRAM_IDS.metadata),
        )
    )[0];
}

export const decodeMetadata = (buffer: Buffer): Metadata => {
    const metadata = deserializeUnchecked(
        METADATA_SCHEMA,
        Metadata,
        buffer
    ) as Metadata;

    metadata.data.name = metadata.data.name.replace(/\0/g, '');
    metadata.data.symbol = metadata.data.symbol.replace(/\0/g, '');
    metadata.data.uri = metadata.data.uri.replace(/\0/g, '');
    metadata.data.name = metadata.data.name.replace(/\0/g, '');
    return metadata;
};

export const extendBorsh = () => {
    (BinaryReader.prototype as any).readPubkey = function () {
        const reader = this as unknown as BinaryReader;
        const array = reader.readFixedArray(32);
        return new PublicKey(array);
    };

    (BinaryWriter.prototype as any).writePubkey = function (value: any) {
        const writer = this as unknown as BinaryWriter;
        writer.writeFixedArray(value.toBuffer());
    };

    (BinaryReader.prototype as any).readPubkeyAsString = function () {
        const reader = this as unknown as BinaryReader;
        const array = reader.readFixedArray(32);
        return base58.encode(array) as StringPublicKey;
    };

    (BinaryWriter.prototype as any).writePubkeyAsString = function (
        value: StringPublicKey
    ) {
        const writer = this as unknown as BinaryWriter;
        writer.writeFixedArray(base58.decode(value));
    };
};

extendBorsh();

async function fetchMetadataFromPDA(pubkey: PublicKey, url: string) {
    const connection = new Connection(url);
    const metadataKey = (await (getMetadata(pubkey.toBase58())));
    const metadataInfo = (await connection.getAccountInfo(toPublicKey(metadataKey)));

    return metadataInfo;
}

async function fetchMetadata(
    dispatch: Dispatch,
    pubkey: PublicKey,
    cluster: Cluster,
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
        const metadataPromise = (await (fetchMetadataFromPDA(pubkey, url)));

        if (metadataPromise && metadataPromise.data.length > 0) {
            const metadata = decodeMetadata(metadataPromise.data);
            data = { meta: metadata }
        }
        status = FetchStatus.Fetched;
    } catch (error) {
        if (cluster !== Cluster.Custom) {
            reportError(error, { url });
        }
        status = FetchStatus.FetchFailed;
    }
    dispatch({ type: ActionType.Update, url, status, data, key });
}

export function useMetadata(
    address: string
): Cache.CacheEntry<AccountMetadata> | undefined {
    const context = React.useContext(StateContext);

    if (!context) {
        throw new Error(
            `useMetadata must be used within a AccountsProvider`
        );
    }

    return context.entries[address];
}

export function useFetchMetadata() {
    const dispatch = React.useContext(DispatchContext);
    if (!dispatch) {
        throw new Error(
            `useFetchMetadata must be used within a AccountsProvider`
        );
    }

    const { cluster, url } = useCluster();
    return React.useCallback(
        (pubkey: PublicKey) => {
            fetchMetadata(dispatch, pubkey, cluster, url);
        },
        [dispatch, cluster, url]
    );
}