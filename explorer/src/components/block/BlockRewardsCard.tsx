import React from "react";
import { lamportsToSolString } from "utils";
import { ConfirmedBlock, PublicKey } from "@solana/web3.js";
import { Address } from "components/common/Address";

export function BlockRewardsCard({ block }: { block: ConfirmedBlock }) {
  if (block.rewards.length < 1) {
    return null;
  }

  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Block Rewards</h3>
      </div>

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">Address</th>
              <th className="text-muted">Type</th>
              <th className="text-muted">Amount</th>
              <th className="text-muted">New Balance</th>
              <th className="text-muted">Percent Change</th>
            </tr>
          </thead>
          <tbody>
            {block.rewards.map((reward) => {
              let percentChange;
              if (reward.postBalance !== null && reward.postBalance !== 0) {
                percentChange = (
                  (Math.abs(reward.lamports) /
                    (reward.postBalance - reward.lamports)) *
                  100
                ).toFixed(9);
              }
              return (
                <tr key={reward.pubkey + reward.rewardType}>
                  <td>
                    <Address pubkey={new PublicKey(reward.pubkey)} link />
                  </td>
                  <td>{reward.rewardType}</td>
                  <td>{lamportsToSolString(reward.lamports)}</td>
                  <td>
                    {reward.postBalance
                      ? lamportsToSolString(reward.postBalance)
                      : "-"}
                  </td>
                  <td>{percentChange ? percentChange + "%" : "-"}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
