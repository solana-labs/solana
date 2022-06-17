import React from "react";
import Link from "next/link";
import { useRouter } from "next/router";
import { TransactionSignature } from "@solana/web3.js";
import { clusterPath } from "src/utils/url";
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
  const router = useRouter();
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
            <Link href={clusterPath(`/tx/${signature}`, router.asPath)} passHref>
              <a className={truncate ? "text-truncate signature-truncate" : ""}>
                {signatureLabel}
              </a>
            </Link>
          ) : (
            signatureLabel
          )}
        </span>
      </Copyable>
    </div>
  );
}
