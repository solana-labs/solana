import { IMetadataExtension, Metadata } from "metaplex/classes";
import { StringPublicKey } from "metaplex/types";
import { useEffect, useState } from "react";

enum ArtFetchStatus {
  ReadyToFetch,
  Fetching,
  FetchFailed,
  FetchSucceeded,
}

const cachedImages = new Map<string, string>();
export const useCachedImage = (uri: string) => {
  const [cachedBlob, setCachedBlob] = useState<string | undefined>(undefined);
  const [fetchStatus, setFetchStatus] = useState<ArtFetchStatus>(
    ArtFetchStatus.ReadyToFetch
  );

  useEffect(() => {
    if (!uri) {
      return;
    }

    if (fetchStatus === ArtFetchStatus.FetchFailed) {
      setCachedBlob(uri);
      return;
    }

    const result = cachedImages.get(uri);
    if (result) {
      setCachedBlob(result);
      return;
    }

    if (fetchStatus === ArtFetchStatus.ReadyToFetch) {
      (async () => {
        setFetchStatus(ArtFetchStatus.Fetching);
        let response: Response;
        try {
          response = await fetch(uri, { cache: "force-cache" });
        } catch {
          try {
            response = await fetch(uri, { cache: "reload" });
          } catch {
            if (uri?.startsWith("http")) {
              setCachedBlob(uri);
            }
            setFetchStatus(ArtFetchStatus.FetchFailed);
            return;
          }
        }

        const blob = await response.blob();
        const blobURI = URL.createObjectURL(blob);
        cachedImages.set(uri, blobURI);
        setCachedBlob(blobURI);
        setFetchStatus(ArtFetchStatus.FetchSucceeded);
      })();
    }
  }, [uri, setCachedBlob, fetchStatus, setFetchStatus]);

  return { cachedBlob };
};

export const useExtendedArt = (id: StringPublicKey, metadata: Metadata) => {
  const [data, setData] = useState<IMetadataExtension>();

  useEffect(() => {
    if (id && !data) {
      if (metadata.data.uri) {
        const uri = metadata.data.uri;

        const processJson = (extended: any) => {
          if (!extended || extended?.properties?.files?.length === 0) {
            return;
          }

          if (extended?.image) {
            extended.image = extended.image.startsWith("http")
              ? extended.image
              : `${metadata.data.uri}/${extended.image}`;
          }

          return extended;
        };

        try {
          fetch(uri)
            .then(async (_) => {
              try {
                const data = await _.json();
                try {
                  localStorage.setItem(uri, JSON.stringify(data));
                } catch {
                  // ignore
                }
                setData(processJson(data));
              } catch {
                return undefined;
              }
            })
            .catch(() => {
              return undefined;
            });
        } catch (ex) {
          console.error(ex);
        }
      }
    }
  }, [id, data, setData, metadata.data.uri]);

  return { data };
};
