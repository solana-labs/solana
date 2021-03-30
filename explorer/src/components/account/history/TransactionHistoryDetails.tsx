import React from "react";
import { PublicKey } from "@solana/web3.js";
import { Signature } from "components/common/Signature";
import { Slot } from "components/common/Slot";
import { displayTimestamp } from "utils/date";
import { SlotRow } from "../TransactionHistoryCardWrapper";

export function TransactionHistoryDetails({
  pubkey,
  slotRows,
}: {
  pubkey: PublicKey;
  slotRows: SlotRow[];
}) {
  const hasTimestamps = !!slotRows.find((element) => !!element.blockTime);
  const detailsList: React.ReactNode[] = slotRows.map(
    ({ slot, signature, blockTime, failed }) => {
      return (
        <tr
          key={signature}
          className={`${failed && "transaction-failed"}`}
          title={`${failed && "Transaction Failed"}`}
        >
          <td className="w-1">
            <Slot slot={slot} link />
          </td>

          <td>
            <Signature signature={signature} link />
          </td>

          {hasTimestamps && (
            <td className="text-muted">
              {blockTime ? displayTimestamp(blockTime * 1000, true) : "---"}
            </td>
          )}
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
            <th className="text-muted">Transaction Signature</th>
            {hasTimestamps && <th className="text-muted">Timestamp</th>}
          </tr>
        </thead>
        <tbody className="list">{detailsList}</tbody>
      </table>
    </div>
  );
}
