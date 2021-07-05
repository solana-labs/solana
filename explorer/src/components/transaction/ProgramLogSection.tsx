import React from "react";
import { SignatureProps } from "pages/TransactionDetailsPage";
import { useTransactionDetails } from "providers/transactions";

export function ProgramLogSection({ signature }: SignatureProps) {
  const details = useTransactionDetails(signature);
  const logMessages = details?.data?.transaction?.meta?.logMessages;

  if (!logMessages || logMessages.length < 1) {
    return null;
  }

  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body">
            <h3 className="card-header-title">Program Log</h3>
          </div>
        </div>
      </div>
      <div className="card">
        <ul className="log-messages">
          {logMessages.map((message, key) => (
            <li key={key}>{message.replace(/^Program log: /, "")}</li>
          ))}
        </ul>
      </div>
    </>
  );
}
