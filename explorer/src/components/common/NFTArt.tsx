import { useEffect, useState } from "react";
import { Stream } from "@cloudflare/stream-react";
import { PublicKey } from "@solana/web3.js";
import {
  programs,
  MetadataJson,
  MetaDataJsonCategory,
  MetadataJsonFile,
} from "@metaplex/js";
import ContentLoader from "react-content-loader";
import ErrorLogo from "img/logos-solana/dark-solana-logo.svg";
import { getLast } from "utils";

export const MAX_TIME_LOADING_IMAGE = 5000; /* 5 seconds */

const LoadingPlaceholder = () => (
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

const ErrorPlaceHolder = () => (
  <img src={ErrorLogo} width="120" height="120" alt="Solana Logo" />
);

const ViewOriginalArtContentLink = ({ src }: { src: string }) => {
  if (!src) {
    return null;
  }

  return (
    <h6 className={"header-pretitle d-flex justify-content-center mt-2"}>
      <a href={src}>VIEW ORIGINAL</a>
    </h6>
  );
};

export const CachedImageContent = ({ uri }: { uri?: string }) => {
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [showError, setShowError] = useState<boolean>(false);
  const [timeout, setTimeout] = useState<NodeJS.Timeout | undefined>(undefined);

  useEffect(() => {
    // Set the timeout if we don't have a valid uri
    if (!uri && !timeout) {
      setTimeout(setInterval(() => setShowError(true), MAX_TIME_LOADING_IMAGE));
    }

    // We have a uri - clear the timeout
    if (uri && timeout) {
      clearInterval(timeout);
    }

    return () => {
      if (timeout) {
        clearInterval(timeout);
      }
    };
  }, [uri, setShowError, timeout, setTimeout]);

  const { cachedBlob } = useCachedImage(uri || "");

  return (
    <>
      {showError ? (
        <div className={"art-error-image-placeholder"}>
          <ErrorPlaceHolder />
          <h6 className={"header-pretitle mt-2"}>Error Loading Image</h6>
        </div>
      ) : (
        <>
          {isLoading && <LoadingPlaceholder />}
          <div className={`${isLoading ? "d-none" : "d-block"}`}>
            <img
              className={`rounded mx-auto ${isLoading ? "d-none" : "d-block"}`}
              src={cachedBlob}
              alt={"nft"}
              style={{
                width: 150,
                maxHeight: 200,
              }}
              onLoad={() => {
                setIsLoading(false);
              }}
              onError={() => {
                setShowError(true);
              }}
            />
            {uri && <ViewOriginalArtContentLink src={uri} />}
          </div>
        </>
      )}
    </>
  );
};

const VideoArtContent = ({
  files,
  uri,
  animationURL,
}: {
  files?: (MetadataJsonFile | string)[];
  uri?: string;
  animationURL?: string;
}) => {
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
      <div className={"d-block"}>
        <Stream
          src={likelyVideo.replace("https://watch.videodelivery.net/", "")}
          loop={true}
          height={180}
          width={320}
          controls={false}
          style={{ borderRadius: 12 }}
          videoDimensions={{
            videoWidth: 320,
            videoHeight: 180,
          }}
          autoplay={true}
          muted={true}
        />
        <ViewOriginalArtContentLink
          src={likelyVideo.replace("https://watch.videodelivery.net/", "")}
        />
      </div>
    ) : (
      <div className={"d-block"}>
        <video
          playsInline={true}
          autoPlay={true}
          muted={true}
          controls={true}
          controlsList="nodownload"
          style={{ borderRadius: 12, width: 320, height: 180 }}
          loop={true}
          poster={uri}
        >
          {likelyVideo && <source src={likelyVideo} type="video/mp4" />}
          {animationURL && <source src={animationURL} type="video/mp4" />}
          {files
            ?.filter((f) => typeof f !== "string")
            .map((f: any, index: number) => (
              <source key={index} src={f.uri} type={f.type} />
            ))}
        </video>
        {(likelyVideo || animationURL) && (
          <ViewOriginalArtContentLink src={(likelyVideo || animationURL)!} />
        )}
      </div>
    );

  return content;
};

const HTMLContent = ({
  animationUrl,
  files,
}: {
  animationUrl?: string;
  files?: (MetadataJsonFile | string)[];
}) => {
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [showError, setShowError] = useState<boolean>(false);
  const htmlURL =
    files && files.length > 0 && typeof files[0] === "string"
      ? files[0]
      : animationUrl;

  return (
    <>
      {showError ? (
        <div className={"art-error-image-placeholder"}>
          <ErrorPlaceHolder />
          <h6 className={"header-pretitle mt-2"}>Error Loading Image</h6>
        </div>
      ) : (
        <>
          {!isLoading && <LoadingPlaceholder />}
          <div className={`${isLoading ? "d-block" : "d-none"}`}>
            <iframe
              allow="accelerometer; autoplay; encrypted-media; gyroscope; picture-in-picture"
              title={"html-content"}
              sandbox="allow-scripts"
              frameBorder="0"
              src={htmlURL}
              style={{ width: 320, height: 180, borderRadius: 12 }}
              onLoad={() => {
                setIsLoading(true);
              }}
              onError={() => {
                setShowError(true);
              }}
            ></iframe>
            {htmlURL && <ViewOriginalArtContentLink src={htmlURL} />}
          </div>
        </>
      )}
    </>
  );
};

export const ArtContent = ({
  metadata,
  category,
  pubkey,
  uri,
  animationURL,
  files,
  data,
}: {
  metadata: programs.metadata.MetadataData;
  category?: MetaDataJsonCategory;
  pubkey?: PublicKey | string;
  uri?: string;
  animationURL?: string;
  files?: (MetadataJsonFile | string)[];
  data: MetadataJson | undefined;
}) => {
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
      <VideoArtContent files={files} uri={uri} animationURL={animationURL} />
    ) : category === "html" || animationUrlExt === "html" ? (
      <HTMLContent animationUrl={animationURL} files={files} />
    ) : (
      <CachedImageContent uri={uri} />
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
