import axios from "axios";
import pLimit from "p-limit";
import { Connection, PublicKey } from "@solana/web3.js";
import bs58 from "bs58";
import { NftokenTypes } from "./nftoken-types";

export const NFTOKEN_ADDRESS = "nftokf9qcHSYkVSP3P2gUMmV6d4AwjMueXgUu43HyLL";

const nftokenAccountDiscInHex = "21b45b35ec0f3f61";

export namespace NftokenFetcher {
  export const getNftsInCollection = async ({
    collection,
    rpcUrl,
  }: {
    collection: string;
    rpcUrl: string;
  }): Promise<NftokenTypes.NftInfo[]> => {
    const connection = new Connection(rpcUrl);
    const accounts = await connection.getProgramAccounts(
      new PublicKey(NFTOKEN_ADDRESS),
      {
        filters: [
          {
            memcmp: {
              offset: 0,
              bytes: bs58.encode(Buffer.from(nftokenAccountDiscInHex, "hex")),
            },
          },
          {
            memcmp: {
              offset:
                8 + // discriminator
                1 + // version
                32 + // holder
                32 + // authority
                1, // authority_can_update
              bytes: collection,
            },
          },
        ],
      }
    );

    const parsed_accounts: NftokenTypes.NftAccount[] = accounts.flatMap(
      (account) => {
        const parsed = NftokenTypes.nftAccountLayout.decode(
          account.account.data
        );

        if (!parsed) {
          return [];
        }
        return {
          address: account.pubkey.toBase58(),
          holder: parsed.holder,
          authority: parsed.authority,
          authority_can_update: Boolean(parsed.authority_can_update),

          collection: parsed.collection,
          delegate: parsed.delegate,

          metadata_url: parsed.metadata_url,
        };
      }
    );

    const metadata_urls = parsed_accounts.map((a) => a.metadata_url);
    const metadataMap = await getMetadataMap({ urls: metadata_urls });

    const nfts = parsed_accounts.map((account) => ({
      ...account,
      ...metadataMap.get(account.metadata_url),
    }));
    nfts.sort();
    return nfts.sort((a, b) => {
      if (a.name && b.name) {
        return a.name < b.name ? -1 : 1;
      }

      if (a.name) {
        return 1;
      }

      if (b.name) {
        return -1;
      }

      return a.address < b.address ? 1 : -1;
    });
  };

  export const getMetadata = async ({
    url,
  }: {
    url: string | null | undefined;
  }): Promise<NftokenTypes.Metadata | null> => {
    if (!url) {
      return null;
    }

    const metadataMap = await getMetadataMap({
      urls: [url],
    });
    return metadataMap.get(url) ?? null;
  };

  export const getMetadataMap = async ({
    urls: _urls,
  }: {
    urls: Array<string | null | undefined>;
  }): Promise<Map<string, NftokenTypes.Metadata | null>> => {
    const urls = Array.from(
      new Set(_urls.filter((url): url is string => Boolean(url)))
    );

    const metadataMap = new Map<string, NftokenTypes.Metadata | null>();

    const limit = pLimit(5);
    const promises = urls.map((url) =>
      limit(async () => {
        try {
          const { data } = await axios.get(url, {
            timeout: 5_000,
          });
          metadataMap.set(url, {
            name: data.name ?? "",
            description: data.description ?? null,
            image: data.image ?? "",
            traits: data.traits ?? [],
            animation_url: data.animation_url ?? null,
            external_url: data.external_url ?? null,
          });
        } catch {
          metadataMap.set(url, null);
        }
      })
    );
    await Promise.all(promises);

    return metadataMap;
  };
}
