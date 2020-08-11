import React, { useState } from "react";
import { Link } from "react-router-dom";
import { TransactionSignature } from "@solana/web3.js";
import { clusterPath } from "utils/url";

type CopyState = "copy" | "copied";
type Props = {
  signature: TransactionSignature;
  alignRight?: boolean;
  link?: boolean;
};

export function Signature({ signature, alignRight, link }: Props) {
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
          <Link className="" to={clusterPath(`/tx/${signature}`)}>
            {signature}
            <span className="fe fe-external-link ml-2"></span>
          </Link>
        ) : (
          signature
        )}
      </span>
    </div>
  );
}
