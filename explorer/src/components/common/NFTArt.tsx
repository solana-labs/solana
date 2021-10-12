import React, { Ref, useCallback, useEffect, useState } from "react";
import { Stream, StreamPlayerApi } from "@cloudflare/stream-react";
import { PublicKey } from "@solana/web3.js";
import {
  MetadataData,
  MetadataJson,
  MetaDataJsonCategory,
  MetadataJsonFile,
} from "@metaplex/js";
import ContentLoader from "react-content-loader";
import { getLast, pubkeyToString } from "utils";

const Placeholder = () => (
  <ContentLoader
    viewBox="0 0 212 200"
    height={150}
    width={150}
    backgroundColor="transparent"
  >
    <circle cx="86" cy="100" r="8" />
    <circle cx="106" cy="100" r="8" />
    <circle cx="126" cy="100" r="8" />
  </ContentLoader>
);

const CachedImageContent = ({
  uri,
}: {
  uri?: string;
  className?: string;
  preview?: boolean;
  style?: React.CSSProperties;
}) => {
  const [loaded, setLoaded] = useState<boolean>(false);
  const { cachedBlob } = useCachedImage(uri || "");

  return (
    <>
      {!loaded && <Placeholder />}
      <img
        className={`rounded mx-auto ${loaded ? "d-block" : "d-none"}`}
        src={cachedBlob}
        loading="lazy"
        alt={"nft"}
        style={{
          width: 150,
          height: "auto",
        }}
        onLoad={() => {
          setLoaded(true);
        }}
        onError={() => {
          setLoaded(true);
        }}
      />
    </>
  );
};

const VideoArtContent = ({
  className,
  style,
  files,
  uri,
  animationURL,
  active,
}: {
  className?: string;
  style?: React.CSSProperties;
  files?: (MetadataJsonFile | string)[];
  uri?: string;
  animationURL?: string;
  active?: boolean;
}) => {
  const [playerApi, setPlayerApi] = useState<StreamPlayerApi>();

  const playerRef = useCallback(
    (ref) => {
      setPlayerApi(ref);
    },
    [setPlayerApi]
  );

  useEffect(() => {
    if (playerApi) {
      playerApi.currentTime = 0;
    }
  }, [active, playerApi]);

  const likelyVideo = (files || []).filter((f, index, arr) => {
    if (typeof f !== "string") {
      return false;
    }

    // TODO: filter by fileType
    return arr.length >= 2 ? index === 1 : index === 0;
  })?.[0] as string;

  const content =
    likelyVideo &&
    likelyVideo.startsWith("https://watch.videodelivery.net/") ? (
      <div className={`${className} square`}>
        <Stream
          streamRef={(e: any) => playerRef(e)}
          src={likelyVideo.replace("https://watch.videodelivery.net/", "")}
          loop={true}
          height={150}
          width={150}
          controls={false}
          style={{ borderRadius: 12 }}
          videoDimensions={{
            videoHeight: 150,
            videoWidth: 150,
          }}
          autoplay={true}
          muted={true}
        />
      </div>
    ) : (
      <video
        className={className}
        playsInline={true}
        autoPlay={true}
        muted={true}
        controls={true}
        controlsList="nodownload"
        style={{ borderRadius: 12, ...style }}
        loop={true}
        poster={uri}
      >
        {likelyVideo && (
          <source src={likelyVideo} type="video/mp4" style={style} />
        )}
        {animationURL && (
          <source src={animationURL} type="video/mp4" style={style} />
        )}
        {files
          ?.filter((f) => typeof f !== "string")
          .map((f: any) => (
            <source src={f.uri} type={f.type} style={style} />
          ))}
      </video>
    );

  return content;
};

const HTMLContent = ({
  animationUrl,
  className,
  style,
  files,
}: {
  animationUrl?: string;
  className?: string;
  style?: React.CSSProperties;
  files?: (MetadataJsonFile | string)[];
}) => {
  const [loaded, setLoaded] = useState<boolean>(false);
  const htmlURL =
    files && files.length > 0 && typeof files[0] === "string"
      ? files[0]
      : animationUrl;

  return (
    <>
      {!loaded && <Placeholder />}
      <iframe
        allow="accelerometer; autoplay; encrypted-media; gyroscope; picture-in-picture"
        title={"html-content"}
        sandbox="allow-scripts"
        frameBorder="0"
        src={htmlURL}
        className={`${className} ${loaded ? "d-block" : "d-none"}`}
        style={{ width: 150, borderRadius: 12, ...style }}
        onLoad={() => {
          setLoaded(true);
        }}
        onError={() => {
          setLoaded(true);
        }}
      ></iframe>
    </>
  );
};

export const ArtContent = ({
  metadata,
  category,
  className,
  preview,
  style,
  active,
  pubkey,
  uri,
  animationURL,
  files,
}: {
  metadata: MetadataData;
  category?: MetaDataJsonCategory;
  className?: string;
  preview?: boolean;
  style?: React.CSSProperties;
  width?: number;
  height?: number;
  ref?: Ref<HTMLDivElement>;
  active?: boolean;
  pubkey?: PublicKey | string;
  uri?: string;
  animationURL?: string;
  files?: (MetadataJsonFile | string)[];
}) => {
  const id = pubkeyToString(pubkey);

  const { data } = useExtendedArt(id, metadata);

  if (pubkey && data) {
    uri = data.image;
    animationURL = data.animation_url;
  }

  if (pubkey && data?.properties) {
    files = data.properties.files;
    category = data.properties.category;
  }

  animationURL = animationURL || "";

  const animationUrlExt = new URLSearchParams(
    getLast(animationURL.split("?"))
  ).get("ext");

  const content =
    category === "video" ? (
      <VideoArtContent
        className={className}
        style={style}
        files={files}
        uri={uri}
        animationURL={animationURL}
        active={active}
      />
    ) : animationUrlExt === "html" ? (
      <HTMLContent
        animationUrl={animationURL}
        className={className}
        style={style}
        files={files}
      />
    ) : (
      <CachedImageContent
        uri={uri}
        className={className}
        preview={preview}
        style={style}
      />
    );

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      {content}
    </div>
  );
};

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

export const useExtendedArt = (id: string, metadata: MetadataData) => {
  const [data, setData] = useState<MetadataJson>();

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
