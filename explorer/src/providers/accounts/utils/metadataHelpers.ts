import { AccountInfo, Connection, PublicKey } from "@solana/web3.js";
import {
  Edition,
  MasterEditionV1,
  MasterEditionV2,
  Metadata,
  METADATA_SCHEMA,
} from "metaplex/classes";
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
import { TOKEN_PROGRAM_ID } from "providers/accounts/tokens";

let STORE: PublicKey | undefined;

export type EditionData = {
  masterEdition?: MasterEditionV1 | MasterEditionV2;
  edition?: Edition;
};

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
  url: string
): Promise<Metadata | undefined> {
  const connection = new Connection(url, "confirmed");
  const metadataKey = await generatePDA(pubkey);
  const accountInfo = await connection.getAccountInfo(toPublicKey(metadataKey));

  if (accountInfo && accountInfo.data.length > 0) {
    if (!isMetadataAccount(accountInfo)) return;

    if (isMetadataV1Account(accountInfo)) {
      const metadata = decodeMetadata(accountInfo.data);

      if (isValidHttpUrl(metadata.data.uri)) {
        return metadata;
      }
    }
  }
}

export async function getEditionData(
  pubkey: PublicKey,
  url: string
): Promise<EditionData | undefined> {
  const connection = new Connection(url, "confirmed");
  const editionKey = await generatePDA(pubkey, true /* addEditionToSeeds */);
  const accountInfo = await connection.getAccountInfo(toPublicKey(editionKey));

  if (accountInfo && accountInfo.data.length > 0) {
    if (!isMetadataAccount(accountInfo)) return;

    if (isMasterEditionAccount(accountInfo)) {
      return {
        masterEdition: decodeMasterEdition(accountInfo.data),
        edition: undefined,
      };
    }

    // This is an Edition NFT. Pull the Parent (MasterEdition)
    if (isEditionV1Account(accountInfo)) {
      const edition = decodeEdition(accountInfo.data);
      const masterEditionAccountInfo = await connection.getAccountInfo(
        toPublicKey(edition.parent)
      );

      if (
        masterEditionAccountInfo &&
        masterEditionAccountInfo.data.length > 0 &&
        isMasterEditionAccount(masterEditionAccountInfo)
      ) {
        return {
          masterEdition: decodeMasterEdition(masterEditionAccountInfo.data),
          edition,
        };
      }
    }
  }

  return;
}

async function generatePDA(
  tokenMint: PublicKey,
  addEditionToSeeds: boolean = false
): Promise<PublicKey> {
  const PROGRAM_IDS = programIds();

  const metadataSeeds = [
    Buffer.from(METADATA_PREFIX),
    toPublicKey(PROGRAM_IDS.metadata).toBuffer(),
    tokenMint.toBuffer(),
  ];

  if (addEditionToSeeds) {
    metadataSeeds.push(Buffer.from("edition"));
  }

  return (
    await PublicKey.findProgramAddress(
      metadataSeeds,
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

  // Remove any trailing null characters from the deserialized strings
  metadata.data.name = metadata.data.name.replace(/\0/g, "");
  metadata.data.symbol = metadata.data.symbol.replace(/\0/g, "");
  metadata.data.uri = metadata.data.uri.replace(/\0/g, "");
  metadata.data.name = metadata.data.name.replace(/\0/g, "");
  return metadata;
};

export const decodeMasterEdition = (
  buffer: Buffer
): MasterEditionV1 | MasterEditionV2 => {
  if (buffer[0] === MetadataKey.MasterEditionV1) {
    return deserializeUnchecked(
      METADATA_SCHEMA,
      MasterEditionV1,
      buffer
    ) as MasterEditionV1;
  } else {
    return deserializeUnchecked(
      METADATA_SCHEMA,
      MasterEditionV2,
      buffer
    ) as MasterEditionV2;
  }
};

export const decodeEdition = (buffer: Buffer) => {
  return deserializeUnchecked(METADATA_SCHEMA, Edition, buffer) as Edition;
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

function isValidHttpUrl(text: string) {
  try {
    const url = new URL(text);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch (_) {
    return false;
  }
}

// Required to properly serialize and deserialize pubKeyAsString types
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
