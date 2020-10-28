import bs58 from "bs58";
import React from "react";
import { TableCardBody } from "components/common/TableCardBody";
import { useBlock, useFetchBlock, FetchStatus } from "providers/block";
import { Signature } from "components/common/Signature";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { Slot } from "components/common/Slot";

export function BlockHistoryCard({ slot }: { slot: string }) {
  const ConfirmedBlock = useBlock(slot);
  const fetchBlock = useFetchBlock();
  const refresh = () => fetchBlock(slot);

  React.useEffect(() => {
    if (!ConfirmedBlock) refresh();
  }, [ConfirmedBlock, slot]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!ConfirmedBlock) {
    return null;
  }

  if (ConfirmedBlock?.data === undefined) {
    if (ConfirmedBlock.status === FetchStatus.Fetching) {
      return <LoadingCard message="Loading block" />;
    }

    return <ErrorCard retry={refresh} text="Failed to fetch block" />;
  }

  if (ConfirmedBlock.status === FetchStatus.FetchFailed) {
    return <ErrorCard retry={refresh} text="Failed to fetch block" />;
  }

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
              <Slot slot={Number(slot)} />
            </td>
          </tr>
          <tr>
            <td className="w-100">Parent Slot</td>
            <td className="text-lg-right text-monospace">
              <Slot slot={ConfirmedBlock.data.parentSlot} />
            </td>
          </tr>
          <tr>
            <td className="w-100">Blockhash</td>
            <td className="text-lg-right text-monospace">
              <span>{ConfirmedBlock.data.blockhash}</span>
            </td>
          </tr>
          <tr>
            <td className="w-100">Previous Blockhash</td>
            <td className="text-lg-right text-monospace">
              <span>{ConfirmedBlock.data.previousBlockhash}</span>
            </td>
          </tr>
        </TableCardBody>
      </div>

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
              {ConfirmedBlock.data.transactions.map((tx) => {
                const signature = bs58.encode(
                  tx.transaction.signature
                    ? tx.transaction.signature
                    : new Buffer("")
                );
                return (
                  <tr key={signature}>
                    <td>
                      <span className={`badge badge-soft-success`}>
                        success
                      </span>
                    </td>

                    <td>
                      <Signature signature={signature} link />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}
