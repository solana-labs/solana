import useSWR, { SWRResponse } from "swr";
import { useCluster } from "../../../providers/cluster";
import { NftokenFetcher } from "./nftoken";
import { NftokenTypes } from "./nftoken-types";

const getCollectionNftsFetcher = async (
  _method: string,
  collectionAddress: string,
  url: string
) => {
  return await NftokenFetcher.getNftsInCollection({
    collection: collectionAddress,
    rpcUrl: url,
  });
};

export const useCollectionNfts = ({
  collectionAddress,
}: {
  collectionAddress: string;
}): {
  // We can be confident that data will be nonnull even if the request fails,
  // if we defined fallbackData in the config.
  data: NftokenTypes.NftInfo[];
  error: any;
  mutate: SWRResponse<NftokenTypes.NftInfo[], never>["mutate"];
} => {
  const { url } = useCluster();

  const swrKey = ["getNftsInCollection", collectionAddress, url];
  const { data, error, mutate } = useSWR(swrKey, getCollectionNftsFetcher, {
    suspense: true,
  });
  // Not nullable since we use suspense
  return { data: data!, error, mutate };
};

const getMetadataFetcher = async (metadataUrl: string) => {
  return await NftokenFetcher.getMetadata({ url: metadataUrl });
};

export const useNftokenMetadata = (
  metadataUrl: string | null | undefined
): {
  data: NftokenTypes.Metadata | null;
  error: any;
  mutate: SWRResponse<NftokenTypes.Metadata | null, never>["mutate"];
} => {
  const swrKey = [metadataUrl];
  const { data, error, mutate } = useSWR(swrKey, getMetadataFetcher, {
    suspense: true,
  });
  // Not nullable since we use suspense
  return { data: data!, error, mutate };
};
