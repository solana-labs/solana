import {
  EditionData,
  MasterEdition,
  MasterEditionData,
  Metadata,
  MetadataKey,
} from "@metaplex/js";
import { Connection } from "@solana/web3.js";

export type EditionInfo = {
  masterEdition?: MasterEditionData;
  edition?: EditionData;
};

export default async function getEditionInfo(
  metadata: Metadata,
  connection: Connection
): Promise<EditionInfo> {
  try {
    const edition = (await metadata.getEdition(connection)).data;

    if (edition) {
      if (
        edition.key === MetadataKey.MasterEditionV1 ||
        edition.key === MetadataKey.MasterEditionV2
      ) {
        return {
          masterEdition: edition as MasterEditionData,
          edition: undefined,
        };
      }

      // This is an Edition NFT. Pull the Parent (MasterEdition)
      const masterEdition = (
        await MasterEdition.load(connection, (edition as EditionData).parent)
      ).data;
      if (masterEdition) {
        return {
          masterEdition,
          edition: edition as EditionData,
        };
      }
    }
  } catch {
    /* ignore */
  }

  return {
    masterEdition: undefined,
    edition: undefined,
  };
}
