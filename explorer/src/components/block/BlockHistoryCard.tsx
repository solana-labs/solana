import React from "react";
import { ConfirmedBlock } from "@safecoin/web3.js";
import { ErrorCard } from "components/common/ErrorCard";
import { Signature } from "components/common/Signature";
import bs58 from "bs58";

export function BlockHistoryCard({ block }: { block: ConfirmedBlock }) {
  if (block.transactions.length === 0) {
    return <ErrorCard text="This block has no transactions" />;
  }

  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Block Transactions</h3>
      </div>

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">Result</th>
              <th className="text-muted">Transaction Signature</th>
            </tr>
          </thead>
          <tbody className="list">
            {block.transactions.map((tx, i) => {
              let statusText;
              let statusClass;
              let signature: React.ReactNode;
              if (tx.meta?.err || !tx.transaction.signature) {
                statusClass = "warning";
                statusText = "Failed";
              } else {
                statusClass = "success";
                statusText = "Success";
              }

              if (tx.transaction.signature) {
                signature = (
                  <Signature
                    signature={bs58.encode(tx.transaction.signature)}
                    link
                  />
                );
              }

              return (
                <tr key={i}>
                  <td>
                    <span className={`badge badge-soft-${statusClass}`}>
                      {statusText}
                    </span>
                  </td>

                  <td>{signature}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
