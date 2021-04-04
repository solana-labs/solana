import React from "react";
import { Signature } from "components/common/Signature";
import { Slot } from "components/common/Slot";
import { SlotRow } from "../TransactionHistoryCardWrapper";
import Moment from "react-moment";

export function TransactionHistoryDetails({
  slotRows,
}: {
  slotRows: SlotRow[];
}) {
  const hasTimestamps = !!slotRows.find((element) => !!element.blockTime);
  const detailsList: React.ReactNode[] = slotRows.map(
    ({ slot, signature, blockTime, statusClass, statusText }) => {
      return (
        <tr key={signature}>
          <td>
            <Signature signature={signature} link />
          </td>

          <td className="w-1">
            <Slot slot={slot} link />
          </td>

          {hasTimestamps && (
            <td className="text-muted">
              {blockTime ? <Moment date={blockTime * 1000} fromNow /> : "---"}
            </td>
          )}

          <td>
            <span className={`badge badge-soft-${statusClass}`}>
              {statusText}
            </span>
          </td>
        </tr>
      );
    }
  );

  return (
    <div className="table-responsive mb-0">
      <table className="table table-sm table-nowrap card-table">
        <thead>
          <tr>
            <th className="text-muted">Transaction Signature</th>
            <th className="text-muted w-1">Slot</th>
            {hasTimestamps && <th className="text-muted">Age</th>}
            <th className="text-muted">Result</th>
          </tr>
        </thead>
        <tbody className="list">{detailsList}</tbody>
      </table>
    </div>
  );
}
