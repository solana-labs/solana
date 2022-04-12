import React from "react";
import { Link } from "react-router-dom";
import { PublicKey } from "@solana/web3.js";
import { clusterPath } from "utils/url";
import { displayAddress } from "utils/tx";
import { useCluster } from "providers/cluster";
import { Copyable } from "./Copyable";
import { useTokenRegistry } from "providers/mints/token-registry";
import { useState, useEffect } from "react";
import { Connection, programs } from "@metaplex/js";

type Props = {
  pubkey: PublicKey;
  alignRight?: boolean;
  link?: boolean;
  raw?: boolean;
  truncate?: boolean;
  truncateUnknown?: boolean;
  truncateChars?: number;
  useMetadata?: boolean;
  overrideText?: string;
};

export function Address({
  pubkey,
  alignRight,
  link,
  raw,
  truncate,
  truncateUnknown,
  truncateChars,
  useMetadata,
  overrideText,
}: Props) {
  const address = pubkey.toBase58();
  const { tokenRegistry } = useTokenRegistry();
  const { cluster } = useCluster();

  if (
    truncateUnknown &&
    address === displayAddress(address, cluster, tokenRegistry)
  ) {
    truncate = true;
  }

  let addressLabel = raw
    ? address
    : displayAddress(address, cluster, tokenRegistry);

  var metaplexData = useTokenMetadata(useMetadata, address);
  if (metaplexData && metaplexData.data)
    addressLabel = metaplexData.data.data.name;
  if (truncateChars && addressLabel === address) {
    addressLabel = addressLabel.slice(0, truncateChars) + "…";
  }

  if (overrideText) {
    addressLabel = overrideText;
  }

  const content = (
    <Copyable text={address} replaceText={!alignRight}>
      <span className="font-monospace">
        {link ? (
          <Link
            className={truncate ? "text-truncate address-truncate" : ""}
            to={clusterPath(`/address/${address}`)}
          >
            {addressLabel}
          </Link>
        ) : (
          <span className={truncate ? "text-truncate address-truncate" : ""}>
            {addressLabel}
          </span>
        )}
      </span>
    </Copyable>
  );

  return (
    <>
      <div
        className={`d-none d-lg-flex align-items-center ${
          alignRight ? "justify-content-end" : ""
        }`}
      >
        {content}
      </div>
      <div className="d-flex d-lg-none align-items-center">{content}</div>
    </>
  );
}
export const useTokenMetadata = (
  useMetadata: boolean | undefined,
  pubkey: string
) => {
  const [data, setData] = useState<programs.metadata.MetadataData>();
  var { url } = useCluster();

  useEffect(() => {
    if (!useMetadata) return;
    if (pubkey && !data) {
      programs.metadata.Metadata.getPDA(pubkey)
        .then((pda) => {
          const connection = new Connection(url);
          programs.metadata.Metadata.load(connection, pda)
            .then((metadata) => {
              setData(metadata.data);
            })
            .catch(() => {
              setData(undefined);
            });
        })
        .catch(() => {
          setData(undefined);
        });
    }
  }, [useMetadata, pubkey, url, data, setData]);
  return { data };
};
