import { AccountInfo, Connection, PublicKey } from "@solana/web3.js";
import { Metadata, METADATA_SCHEMA } from "metaplex/classes";
import { MetadataKey, METADATA_PREFIX, StringPublicKey } from "metaplex/types";
import { deserializeUnchecked, BinaryReader, BinaryWriter } from "borsh";
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
  try {
    const connection = new Connection(url);
    const metadataKey = await generatePDA(pubkey.toBase58());
    const accountInfo = await connection.getAccountInfo(
      toPublicKey(metadataKey)
    );

    if (accountInfo && accountInfo.data.length > 0) {
      if (!isMetadataAccount(accountInfo)) return;

      if (isMetadataV1Account(accountInfo)) {
        const metadata = decodeMetadata(accountInfo.data);

        if (isValidHttpUrl(metadata.data.uri)) {
          return metadata;
        }
      }
    }
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
  }
}

export async function hasEdition(
  pubkey: PublicKey,
  cluster: Cluster,
  url: string
) {
  try {
    const connection = new Connection(url);
    const editionkey = await generatePDA(
      pubkey.toBase58(),
      true /* addEditionToSeeds */
    );
    const accountInfo = await connection.getAccountInfo(
      toPublicKey(editionkey)
    );

    if (accountInfo && accountInfo.data.length > 0) {
      if (!isMetadataAccount(accountInfo)) return;

      return (
        isEditionV1Account(accountInfo) || isMasterEditionAccount(accountInfo)
      );
    }
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
  }
}

async function generatePDA(
  tokenMint: StringPublicKey,
  addEditionToSeeds: boolean = false
): Promise<StringPublicKey> {
  const PROGRAM_IDS = programIds();

  const metadataSeeds = [
    Buffer.from(METADATA_PREFIX),
    toPublicKey(PROGRAM_IDS.metadata).toBuffer(),
    toPublicKey(tokenMint).toBuffer(),
  ];

  const editionSeeds = [...metadataSeeds, Buffer.from("edition")];

  return (
    await findProgramAddress(
      addEditionToSeeds ? editionSeeds : metadataSeeds,
      toPublicKey(PROGRAM_IDS.metadata)
    )
  )[0];
}

const decodeMetadata = (buffer: Buffer): Metadata => {
  const metadata = deserializeUnchecked(
    METADATA_SCHEMA,
    Metadata,
    buffer
  ) as Metadata;

  metadata.data.name = metadata.data.name.replace(/\0/g, "");
  metadata.data.symbol = metadata.data.symbol.replace(/\0/g, "");
  metadata.data.uri = metadata.data.uri.replace(/\0/g, "");
  metadata.data.name = metadata.data.name.replace(/\0/g, "");
  return metadata;
};

const isMetadataAccount = (account: AccountInfo<Buffer>) =>
  account.owner.toBase58() === METADATA_PROGRAM_ID;

const isMetadataV1Account = (account: AccountInfo<Buffer>) =>
  account.data[0] === MetadataKey.MetadataV1;

const isEditionV1Account = (account: AccountInfo<Buffer>) =>
  account.data[0] === MetadataKey.EditionV1;

const isMasterEditionAccount = (account: AccountInfo<Buffer>) =>
  account.data[0] === MetadataKey.MasterEditionV1 ||
  account.data[0] === MetadataKey.MasterEditionV2;

export function isValidHttpUrl(text: string) {
  if (text.startsWith("http:") || text.startsWith("https:")) {
    return true;
  }

  return false;
}

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
