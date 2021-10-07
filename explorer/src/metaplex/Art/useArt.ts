import { IMetadataExtension, Metadata } from "metaplex/classes";
import { StringPublicKey } from "metaplex/types";
import { useEffect, useState } from "react";

const cachedImages = new Map<string, string>();
export const useCachedImage = (uri: string) => {
  const [cachedBlob, setCachedBlob] = useState<string | undefined>(undefined);
  const [isLoading, setIsLoading] = useState<boolean>(false);

  useEffect(() => {
    if (!uri) {
      return;
    }

    const result = cachedImages.get(uri);
    if (result) {
      setCachedBlob(result);
      return;
    }

    if (!isLoading) {
      (async () => {
        setIsLoading(true);
        let response: Response;
        try {
          response = await fetch(uri, { cache: "force-cache" });
        } catch {
          try {
            response = await fetch(uri, { cache: "reload" });
          } catch {
            // If external URL, just use the uri
            if (uri?.startsWith("http")) {
              setCachedBlob(uri);
            }
            setIsLoading(false);
            return;
          }
        }

        const blob = await response.blob();
        const blobURI = URL.createObjectURL(blob);
        cachedImages.set(uri, blobURI);
        setCachedBlob(blobURI);
        setIsLoading(false);
      })();
    }
  }, [uri, setCachedBlob, isLoading, setIsLoading]);

  return { cachedBlob, isLoading };
};

export const useExtendedArt = (id: StringPublicKey, metadata: Metadata) => {
  const [data, setData] = useState<IMetadataExtension>();

  useEffect(() => {
    if (id && !data) {
      const USE_CDN = false;
      const routeCDN = (uri: string) => {
        let result = uri;
        if (USE_CDN) {
          result = uri.replace(
            "https://arweave.net/",
            "https://coldcdn.com/api/cdn/bronil/"
          );
        }

        return result;
      };

      if (metadata.data.uri) {
        const uri = routeCDN(metadata.data.uri);

        const processJson = (extended: any) => {
          if (!extended || extended?.properties?.files?.length === 0) {
            return;
          }

          if (extended?.image) {
            const file = extended.image.startsWith("http")
              ? extended.image
              : `${metadata.data.uri}/${extended.image}`;
            extended.image = routeCDN(file);
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
