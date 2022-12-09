import { Slot } from "components/common/Slot";
import React from "react";
import {
  SysvarAccount,
  SlotHashesInfo,
  SlotHashEntry,
} from "validators/accounts/sysvar";

export function SlotHashesCard({
  sysvarAccount,
}: {
  sysvarAccount: SysvarAccount;
}) {
  const slotHashes = sysvarAccount.info as SlotHashesInfo;
  return (
    <div className="card">
      <div className="card-header">
        <div className="row align-items-center">
          <div className="col">
            <h3 className="card-header-title">Slot Hashes</h3>
          </div>
        </div>
      </div>

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="w-1 text-muted">Slot</th>
              <th className="text-muted">Hash</th>
            </tr>
          </thead>
          <tbody className="list">
            {slotHashes.length > 0 &&
              slotHashes.map((entry: SlotHashEntry, index) => {
                return renderAccountRow(entry, index);
              })}
          </tbody>
        </table>
      </div>

      <div className="card-footer">
        <div className="text-muted text-center">
          {slotHashes.length > 0 ? "" : "No hashes found"}
        </div>
      </div>
    </div>
  );
}

const renderAccountRow = (entry: SlotHashEntry, index: number) => {
  return (
    <tr key={index}>
      <td className="w-1 font-monospace">
        <Slot slot={entry.slot} link />
      </td>
      <td className="font-monospace">{entry.hash}</td>
    </tr>
  );
};
