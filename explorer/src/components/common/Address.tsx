import React, { useState } from "react";
import { Link } from "react-router-dom";
import { PublicKey } from "@safecoin/web3.js";
import { clusterPath } from "utils/url";
import { displayAddress } from "utils/tx";
import { useCluster } from "providers/cluster";

type CopyState = "copy" | "copied";
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
  const [state, setState] = useState<CopyState>("copy");
  const address = pubkey.toBase58();
  const { cluster } = useCluster();

  const copyToClipboard = () => navigator.clipboard.writeText(address);
  const handleClick = () =>
    copyToClipboard().then(() => {
      setState("copied");
      setTimeout(() => setState("copy"), 1000);
    });

  const copyIcon =
    state === "copy" ? (
      <span className="fe fe-copy" onClick={handleClick}></span>
    ) : (
      <span className="fe fe-check-circle"></span>
    );

  if (truncateUnknown && address === displayAddress(address, cluster)) {
    truncate = true;
  }

  const content = (
    <>
      <span className="c-pointer font-size-tiny mr-2">{copyIcon}</span>
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
    </>
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
