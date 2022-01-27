import { useState, useEffect } from 'react'
import { getDecentralizedURI } from "../../../utils/url";

export enum ArtFetchStatus {
  ReadyToFetch,
  Fetching,
  FetchFailed,
  FetchSucceeded,
}

export const useCachedImage = (uri?: string) => {
  const [cachedBlob, setCachedBlob] = useState<string | undefined>(undefined);
  const [fetchStatus, setFetchStatus] = useState<ArtFetchStatus>(
    ArtFetchStatus.ReadyToFetch
  );
  const cachedImages = new Map<string, string>();

  useEffect(() => {
    getOrSetCachedBlob()
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [uri]);


  async function getOrSetCachedBlob() {
    if (!uri) return

    if (fetchStatus === ArtFetchStatus.FetchFailed) {
      setCachedBlob(uri);
      return;
    }
    let parsedURI: string | boolean | void | null = null;
    try {
      parsedURI = await getDecentralizedURI(uri)
    } catch (e) {
      console.error(e)
    }
    if(!parsedURI) return
    const result = cachedImages.get(parsedURI);
    if (result) {
      setCachedBlob(result);
      return;
    }

    if (fetchStatus === ArtFetchStatus.ReadyToFetch) {
      setFetchStatus(ArtFetchStatus.Fetching);
      let response: Response;
      try {
        response = await fetch(parsedURI, { cache: "force-cache" });
      } catch {
        try {
          response = await fetch(parsedURI, { cache: "reload" });
        } catch {
          if (parsedURI?.startsWith("http")) {
            setCachedBlob(parsedURI);
          }
          setFetchStatus(ArtFetchStatus.FetchFailed);
          return;
        }
      }

      const blob = await response.blob();
      const blobURI = URL.createObjectURL(blob);
      cachedImages.set(parsedURI, blobURI);
      setCachedBlob(blobURI);
      setFetchStatus(ArtFetchStatus.FetchSucceeded);
    }
  }

  return { cachedBlob };
};
