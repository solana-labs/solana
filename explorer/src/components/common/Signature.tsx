import React from "react";
import { Link } from "react-router-dom";
import { TransactionSignature } from "@solana/web3.js";
import { clusterPath } from "utils/url";
import { Copyable } from "./Copyable";

type Props = {
  signature: TransactionSignature;
  alignRight?: boolean;
  link?: boolean;
  truncate?: boolean;
  truncateChars?: number;
};

export function Signature({
  signature,
  alignRight,
  link,
  truncate,
  truncateChars,
}: Props) {
  let signatureLabel = signature;

  if (truncateChars) {
    signatureLabel = signature.slice(0, truncateChars) + "…";
  }

  return (
    <div
      className={`d-flex align-items-center ${
        alignRight ? "justify-content-end" : ""
      }`}
    >
      <Copyable text={signature} replaceText={!alignRight}>
        <span className="font-monospace">
          {link ? (
            <Link
              className={truncate ? "text-truncate signature-truncate" : ""}
              to={clusterPath(`/tx/${signature}`)}
            >
              {signatureLabel}
            </Link>
          ) : (
            signatureLabel
          )}
        </span>
      </Copyable>
    </div>
  );
}
