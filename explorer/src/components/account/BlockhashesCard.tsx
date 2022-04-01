import React from "react";
import {
  RecentBlockhashesInfo,
  RecentBlockhashesEntry,
} from "validators/accounts/sysvar";

export function BlockhashesCard({
  blockhashes,
}: {
  blockhashes: RecentBlockhashesInfo;
}) {
  return (
    <>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h3 className="card-header-title">Blockhashes</h3>
            </div>
          </div>
        </div>

        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="w-1 text-muted">Recency</th>
                <th className="w-1 text-muted">Blockhash</th>
                <th className="text-muted">Fee Calculator</th>
              </tr>
            </thead>
            <tbody className="list">
              {blockhashes.length > 0 &&
                blockhashes.map((entry: RecentBlockhashesEntry, index) => {
                  return renderAccountRow(entry, index);
                })}
            </tbody>
          </table>
        </div>

        <div className="card-footer">
          <div className="text-muted text-center">
            {blockhashes.length > 0 ? "" : "No blockhashes found"}
          </div>
        </div>
      </div>
    </>
  );
}

const renderAccountRow = (entry: RecentBlockhashesEntry, index: number) => {
  return (
    <tr key={index}>
      <td className="w-1">{index + 1}</td>
      <td className="w-1 font-monospace">{entry.blockhash}</td>
      <td className="">
        {entry.feeCalculator.lamportsPerSignature} lamports per signature
      </td>
    </tr>
  );
};
