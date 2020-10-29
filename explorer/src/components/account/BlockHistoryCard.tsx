import bs58 from "bs58";
import React from "react";
import { TableCardBody } from "components/common/TableCardBody";
import { useBlock, useFetchBlock, FetchStatus } from "providers/block";
import { Signature } from "components/common/Signature";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { Slot } from "components/common/Slot";
import { ClusterStatus, useCluster } from "providers/cluster";

export function BlockHistoryCard({ slot }: { slot: number }) {
  const confirmedBlock = useBlock(slot);
  const fetchBlock = useFetchBlock();
  const { status } = useCluster();
  const refresh = () => fetchBlock(slot);

  // Fetch block on load
  React.useEffect(() => {
    if (!confirmedBlock && status === ClusterStatus.Connected) refresh();
  }, [slot, status]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!confirmedBlock || confirmedBlock.status === FetchStatus.Fetching) {
    return <LoadingCard message="Loading block" />;
  } else if (
    confirmedBlock.data === undefined ||
    confirmedBlock.status === FetchStatus.FetchFailed
  ) {
    return <ErrorCard retry={refresh} text="Failed to fetch block" />;
  } else if (confirmedBlock.data.block === undefined) {
    return <ErrorCard retry={refresh} text={`Block ${slot} was not found`} />;
  }

  const block = confirmedBlock.data.block;
  return (
    <>
      <div className="card">
        <div className="card-header">
          <h3 className="card-header-title mb-0 d-flex align-items-center">
            Overview
          </h3>
        </div>
        <TableCardBody>
          <tr>
            <td className="w-100">Slot</td>
            <td className="text-lg-right text-monospace">
              <Slot slot={slot} />
            </td>
          </tr>
          <tr>
            <td className="w-100">Parent Slot</td>
            <td className="text-lg-right text-monospace">
              <Slot slot={block.parentSlot} link />
            </td>
          </tr>
          <tr>
            <td className="w-100">Blockhash</td>
            <td className="text-lg-right text-monospace">
              <span>{block.blockhash}</span>
            </td>
          </tr>
          <tr>
            <td className="w-100">Previous Blockhash</td>
            <td className="text-lg-right text-monospace">
              <span>{block.previousBlockhash}</span>
            </td>
          </tr>
        </TableCardBody>
      </div>

      {block.transactions.length === 0 ? (
        <ErrorCard text="This block has no transactions" />
      ) : (
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
      )}
    </>
  );
}
