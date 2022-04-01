import { Slot } from "components/common/Slot";
import React from "react";
import { VoteAccount, Vote } from "validators/accounts/vote";

export function VotesCard({ voteAccount }: { voteAccount: VoteAccount }) {
  return (
    <>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h3 className="card-header-title">Vote History</h3>
            </div>
          </div>
        </div>

        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="w-1 text-muted">Slot</th>
                <th className="text-muted">Confirmation Count</th>
              </tr>
            </thead>
            <tbody className="list">
              {voteAccount.info.votes.length > 0 &&
                voteAccount.info.votes
                  .reverse()
                  .map((vote: Vote, index) => renderAccountRow(vote, index))}
            </tbody>
          </table>
        </div>

        <div className="card-footer">
          <div className="text-muted text-center">
            {voteAccount.info.votes.length > 0 ? "" : "No votes found"}
          </div>
        </div>
      </div>
    </>
  );
}

const renderAccountRow = (vote: Vote, index: number) => {
  return (
    <tr key={index}>
      <td className="w-1 font-monospace">
        <Slot slot={vote.slot} link />
      </td>
      <td className="font-monospace">{vote.confirmationCount}</td>
    </tr>
  );
};
