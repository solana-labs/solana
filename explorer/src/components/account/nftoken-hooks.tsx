import useSWR, { SWRResponse } from "swr";
import { useCluster } from "../../providers/cluster";
import { NftokenFetcher } from "../../utils/nftoken";
import { NftokenTypes } from "../../utils/nftoken-types";

export const useCollectionNfts = ({
  collectionAddress,
}: {
  collectionAddress: string;
}): {
  // We can be confident that data will be nonnull even if the request fails,
  // if we defined fallbackData in the config.
  data?: NftokenTypes.NftInfo[];
  error: any;
  mutate: SWRResponse<NftokenTypes.NftInfo[], never>["mutate"];
} => {
  const { url } = useCluster();
  const swrKey = [collectionAddress, "getNftsInCollection", url];
  const { data, error, mutate } = useSWR(swrKey, async () => {
    return await NftokenFetcher.getNftsInCollection({
      collection: collectionAddress,
      rpcUrl: url,
    });
  });
  return { data, error, mutate };
};

export const useNftokenMetadata = (
  metadataUrl: string | null | undefined
): {
  data: NftokenTypes.Metadata | null;
  error: any;
  mutate: SWRResponse<NftokenTypes.Metadata | null, never>["mutate"];
} => {
  const swrKey = [metadataUrl];
  const { data, error, mutate } = useSWR(swrKey, async () => {
    return await NftokenFetcher.getMetadata({ url: metadataUrl });
  });
  return { data: data!, error, mutate };
};
