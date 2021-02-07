import React, { useState } from "react";
import { Link } from "react-router-dom";
import { TransactionSignature } from "@safecoin/web3.js";
import { clusterPath } from "utils/url";

type CopyState = "copy" | "copied";
type Props = {
  signature: TransactionSignature;
  alignRight?: boolean;
  link?: boolean;
  truncate?: boolean;
};

export function Signature({ signature, alignRight, link, truncate }: Props) {
  const [state, setState] = useState<CopyState>("copy");

  const copyToClipboard = () => navigator.clipboard.writeText(signature);
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

  const copyButton = (
    <span className="c-pointer font-size-tiny mr-2">{copyIcon}</span>
  );

  return (
    <div
      className={`d-flex align-items-center ${
        alignRight ? "justify-content-end" : ""
      }`}
    >
      {copyButton}
      <span className="text-monospace">
        {link ? (
          <Link
            className={truncate ? "text-truncate signature-truncate" : ""}
            to={clusterPath(`/tx/${signature}`)}
          >
            {signature}
          </Link>
        ) : (
          signature
        )}
      </span>
    </div>
  );
}
