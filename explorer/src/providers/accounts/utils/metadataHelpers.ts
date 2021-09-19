import { Connection, PublicKey } from "@solana/web3.js";
import { Metadata, METADATA_SCHEMA } from "metaplex/classes";
import { METADATA_PREFIX, StringPublicKey } from "metaplex/types";
import { deserializeUnchecked, BinaryReader, BinaryWriter } from 'borsh';
import base58 from "bs58";
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
import { TOKEN_PROGRAM_ID } from "providers/accounts/tokens";
import { Cluster } from "providers/cluster";

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
    pubkey: PublicKey,
    cluster: Cluster,
    url: string
) {
    let metadata;

    try {
        const metadataPromise = (await (fetchMetadataFromPDA(pubkey, url)));

        if (metadataPromise && metadataPromise.data.length > 0) {
            metadata = decodeMetadata(metadataPromise.data);
        }
    } catch (error) {
        if (cluster !== Cluster.Custom) {
            reportError(error, { url });
        }
    }

    return metadata;
}

async function fetchMetadataFromPDA(pubkey: PublicKey, url: string) {
    const connection = new Connection(url);
    const metadataKey = (await (getMetadataKey(pubkey.toBase58())));
    const metadataInfo = (await connection.getAccountInfo(toPublicKey(metadataKey)));

    return metadataInfo;
}


async function getMetadataKey(
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

const decodeMetadata = (buffer: Buffer): Metadata => {
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

const extendBorsh = () => {
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

