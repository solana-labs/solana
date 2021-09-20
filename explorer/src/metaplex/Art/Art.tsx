import React, { Ref, useCallback, useEffect, useState } from "react";
import { Image } from "antd";
import { MetadataCategory, MetadataFile } from "../types";
import { pubkeyToString } from "../utils";
import { useCachedImage, useExtendedArt } from "./useArt";
import { Stream, StreamPlayerApi } from "@cloudflare/stream-react";
import { PublicKey } from "@solana/web3.js";
import { getLast } from "../utils";
import { Metadata } from "metaplex/classes";

const CachedImageContent = ({
  uri,
  className,
  preview,
  style,
}: {
  uri?: string;
  className?: string;
  preview?: boolean;
  style?: React.CSSProperties;
}) => {
  const { cachedBlob } = useCachedImage(uri || "");

  return (
    <Image
      src={cachedBlob}
      preview={preview}
      wrapperClassName={className}
      loading="lazy"
      wrapperStyle={{ ...style }}
      style={{ width: 150, borderRadius: 12 }}
      height={"auto"}
    />
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
  uri,
  animationUrl,
  className,
  style,
  files,
}: {
  uri?: string;
  animationUrl?: string;
  className?: string;
  style?: React.CSSProperties;
  files?: (MetadataFile | string)[];
}) => {
  const htmlURL =
    files && files.length > 0 && typeof files[0] === "string"
      ? files[0]
      : animationUrl;
  const { isLoading } = useCachedImage(htmlURL || "");

  if (isLoading) {
    return (
      <CachedImageContent uri={uri} className={className} preview={false} />
    );
  }
  return (
    <iframe
      allow="accelerometer; autoplay; encrypted-media; gyroscope; picture-in-picture"
      title={"html-content"}
      sandbox="allow-scripts"
      frameBorder="0"
      src={htmlURL}
      className={className}
      style={{ width: 150, borderRadius: 12, ...style }}
    ></iframe>
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
  metadata: Metadata;
  category?: MetadataCategory;
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
        className={className}
        style={style}
        files={files}
        uri={uri}
        animationURL={animationURL}
        active={active}
      />
    ) : category === "html" || animationUrlExt === "html" ? (
      <HTMLContent
        uri={uri}
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
