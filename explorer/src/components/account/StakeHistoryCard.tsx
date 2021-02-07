import React from "react";
import { lamportsToSafeString } from "utils";
import {
  SysvarAccount,
  StakeHistoryInfo,
  StakeHistoryEntry,
} from "validators/accounts/sysvar";

export function StakeHistoryCard({
  sysvarAccount,
}: {
  sysvarAccount: SysvarAccount;
}) {
  const stakeHistory = sysvarAccount.info as StakeHistoryInfo;
  return (
    <>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h3 className="card-header-title">Stake History</h3>
            </div>
          </div>
        </div>

        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="w-1 text-muted">Epoch</th>
                <th className="text-muted">Effective (SAFE)</th>
                <th className="text-muted">Activating (SAFE)</th>
                <th className="text-muted">Deactivating (SAFE)</th>
              </tr>
            </thead>
            <tbody className="list">
              {stakeHistory.length > 0 &&
                stakeHistory.map((entry: StakeHistoryEntry, index) => {
                  return renderAccountRow(entry, index);
                })}
            </tbody>
          </table>
        </div>

        <div className="card-footer">
          <div className="text-muted text-center">
            {stakeHistory.length > 0 ? "" : "No stake history found"}
          </div>
        </div>
      </div>
    </>
  );
}

const renderAccountRow = (entry: StakeHistoryEntry, index: number) => {
  return (
    <tr key={index}>
      <td className="w-1 text-monospace">{entry.epoch}</td>
      <td className="text-monospace">
        {lamportsToSafeString(entry.stakeHistory.effective)}
      </td>
      <td className="text-monospace">
        {lamportsToSafeString(entry.stakeHistory.activating)}
      </td>
      <td className="text-monospace">
        {lamportsToSafeString(entry.stakeHistory.deactivating)}
      </td>
    </tr>
  );
};
