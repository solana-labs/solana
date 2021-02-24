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
};

export function Signature({ signature, alignRight, link, truncate }: Props) {
  return (
    <div
      className={`d-flex align-items-center ${
        alignRight ? "justify-content-end" : ""
      }`}
    >
      <Copyable text={signature} replaceText={!alignRight}>
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
      </Copyable>
    </div>
  );
}
