import React, { useState } from "react";
import { Link } from "react-router-dom";
import { TransactionSignature } from "@solana/web3.js";
import { clusterPath } from "utils/url";
import { CopyButton } from "./CopyButton";
import { sign } from "crypto";

type CopyState = "copy" | "copied";
type Props = {
  signature: TransactionSignature;
  alignRight?: boolean;
  link?: boolean;
  truncate?: boolean;
};

export function Signature({ signature, alignRight, link, truncate }: Props) {
  return (
    <div
      className={`d-flex align-items-center ${
        alignRight ? "justify-content-end" : ""
      }`}
    >
      <CopyButton text={signature} />
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
