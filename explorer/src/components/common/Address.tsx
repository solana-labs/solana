import React from "react";
import { Link } from "react-router-dom";
import { PublicKey } from "@solana/web3.js";
import { clusterPath } from "utils/url";
import { displayAddress } from "utils/tx";
import { useCluster } from "providers/cluster";
import { Copyable } from "./Copyable";

type Props = {
  pubkey: PublicKey;
  alignRight?: boolean;
  link?: boolean;
  raw?: boolean;
  truncate?: boolean;
  truncateUnknown?: boolean;
};

export function Address({
  pubkey,
  alignRight,
  link,
  raw,
  truncate,
  truncateUnknown,
}: Props) {
  const address = pubkey.toBase58();
  const { cluster } = useCluster();

  if (truncateUnknown && address === displayAddress(address, cluster)) {
    truncate = true;
  }

  const content = (
    <Copyable text={address} replaceText={!alignRight}>
      <span className="text-monospace">
        {link ? (
          <Link
            className={truncate ? "text-truncate address-truncate" : ""}
            to={clusterPath(`/address/${address}`)}
          >
            {raw ? address : displayAddress(address, cluster)}
          </Link>
        ) : (
          <span className={truncate ? "text-truncate address-truncate" : ""}>
            {raw ? address : displayAddress(address, cluster)}
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
