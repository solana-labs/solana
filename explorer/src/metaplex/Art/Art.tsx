import { useCallback, useEffect, useState } from "react";
import { MetadataCategory, MetadataFile } from "../types";
import { pubkeyToString } from "../utils";
import { useCachedImage, useExtendedArt } from "./useArt";
import { Stream, StreamPlayerApi } from "@cloudflare/stream-react";
import { PublicKey } from "@solana/web3.js";
import { getLast } from "../utils";
import { Metadata } from "metaplex/classes";
import ContentLoader from "react-content-loader";
import ErrorLogo from "img/logos-solana/dark-solana-logo.svg";

const MAX_TIME_LOADING_IMAGE = 5000; /* 5 seconds */

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

const CachedImageContent = ({ uri }: { uri?: string }) => {
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
          <img
            className={`rounded mx-auto ${isLoading ? "d-none" : "d-block"}`}
            src={cachedBlob}
            alt={"nft"}
            style={{
              width: 150,
              height: "auto",
            }}
            onLoad={() => {
              setIsLoading(false);
            }}
            onError={() => {
              setShowError(true);
            }}
          />
        </>
      )}
    </>
  );
};

const VideoArtContent = ({
  files,
  uri,
  animationURL,
  active,
}: {
  files?: (MetadataFile | string)[];
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
      <div className={"square"}>
        <Stream
          streamRef={(e: any) => playerRef(e)}
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
      </div>
    ) : (
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
          .map((f: any) => (
            <source src={f.uri} type={f.type} />
          ))}
      </video>
    );

  return content;
};

const HTMLContent = ({
  animationUrl,
  files,
}: {
  animationUrl?: string;
  files?: (MetadataFile | string)[];
}) => {
  const [loaded, setLoaded] = useState<boolean>(false);
  const htmlURL =
    files && files.length > 0 && typeof files[0] === "string"
      ? files[0]
      : animationUrl;

  return (
    <>
      {!loaded && <LoadingPlaceholder />}
      <iframe
        allow="accelerometer; autoplay; encrypted-media; gyroscope; picture-in-picture"
        title={"html-content"}
        sandbox="allow-scripts"
        frameBorder="0"
        src={htmlURL}
        className={`${loaded ? "d-block" : "d-none"}`}
        style={{ width: 320, height: 180, borderRadius: 12 }}
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
  active,
  pubkey,
  uri,
  animationURL,
  files,
}: {
  metadata: Metadata;
  category?: MetadataCategory;
  active?: boolean;
  pubkey?: PublicKey | string;
  uri?: string;
  animationURL?: string;
  files?: (MetadataFile | string)[];
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
        files={files}
        uri={uri}
        animationURL={animationURL}
        active={active}
      />
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
