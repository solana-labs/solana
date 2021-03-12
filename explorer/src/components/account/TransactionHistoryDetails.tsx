import React from "react";
import { PublicKey } from "@solana/web3.js";
import { Signature } from "components/common/Signature";
import { Slot } from "components/common/Slot";
import { displayTimestamp } from "utils/date";
import { SlotRow } from "./TransactionHistoryCardWrapper";

export function TransactionHistoryDetails({
  pubkey,
  slotRows,
}: {
  pubkey: PublicKey;
  slotRows: SlotRow[];
}) {
  const hasTimestamps = !!slotRows.find((element) => !!element.blockTime);
  const detailsList: React.ReactNode[] = slotRows.map(
    ({ slot, signature, blockTime, statusClass, statusText }) => {
      return (
        <tr key={signature}>
          <td className="w-1">
            <Slot slot={slot} link />
          </td>

          {hasTimestamps && (
            <td className="text-muted">
              {blockTime ? displayTimestamp(blockTime * 1000, true) : "---"}
            </td>
          )}

          <td>
            <span className={`badge badge-soft-${statusClass}`}>
              {statusText}
            </span>
          </td>

          <td>
            <Signature signature={signature} link />
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
            <th className="text-muted w-1">Slot</th>
            {hasTimestamps && <th className="text-muted">Timestamp</th>}
            <th className="text-muted">Result</th>
            <th className="text-muted">Transaction Signature</th>
          </tr>
        </thead>
        <tbody className="list">{detailsList}</tbody>
      </table>
    </div>
  );
}
