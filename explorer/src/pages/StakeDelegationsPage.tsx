import React, { useEffect } from "react";

import { useVoteAccounts } from "providers/accounts/vote-accounts";
import { ClusterStatus, useCluster } from "providers/cluster";
import { LoadingCard } from "components/common/LoadingCard";
import { Address } from "components/common/Address";
import { LeaderSchedule, PublicKey } from "@solana/web3.js";

import { useLeaderSchedule } from "providers/accounts/leader-schedule";

const PAGINATION_COUNT = 7;

export function StakeDelegationsPage() {
  const { status } = useCluster();

  const { voteAccounts, fetchVoteAccounts } = useVoteAccounts();
  const { leaderSchedule, fetchLeaderSchedule } = useLeaderSchedule();
  useEffect(() => {
    if (status === ClusterStatus.Connected) {
      fetchVoteAccounts();
      fetchLeaderSchedule();
    }
  }, [status]);

  console.log(leaderSchedule);

  if (voteAccounts && leaderSchedule)
    return (
      <>
        <div>
          <RenderCardSection
            title="Current Validators Data"
            cardData={voteAccounts.current}
          />
          <RenderCardSection
            title="Deliquent Validators Data"
            cardData={voteAccounts.delinquent}
          />
          <RenderLeaderSchedule leaderSchedule={leaderSchedule} />
        </div>
      </>
    );
  else return <LoadingCard message="Loading validators data" />;
}
interface rowData {
  activatedStake: number;
  commission: number;
  lastVote: number;
  nodePubkey: string;
  votePubkey: string;
}
function RenderCardSection({
  title,
  cardData,
}: {
  title: string;
  cardData: rowData[];
}): JSX.Element {
  const [displayedCount, setDisplayedCount] =
    React.useState<number>(PAGINATION_COUNT);
  return (
    <>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h2 className="card-header-title"> {title}</h2>
            </div>
            <div className="col-auto">Total Nodes: {cardData.length}</div>
          </div>
        </div>
        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="text-muted">#</th>
                <th className="text-muted">IDENTITY</th>
                <th className="text-muted text-end">ACTIVE STAKE</th>
                <th className="text-muted text-end">COMMISSION</th>
                <th className="text-muted text-end">LAST VOTE</th>
                <th className="text-muted text-start">VOTE IDENTITY</th>
              </tr>
            </thead>
            <tbody className="list">
              {cardData
                .slice(0, displayedCount)
                .map((data: rowData, index: number): JSX.Element => {
                  return renderRowData(index, data);
                })}
            </tbody>
          </table>
          {cardData.length > PAGINATION_COUNT && (
            <div className="card-footer">
              <button
                className="btn btn-primary w-100"
                onClick={() =>
                  setDisplayedCount(
                    (displayed: number): number => displayed + PAGINATION_COUNT
                  )
                }
              >
                Load More
              </button>
            </div>
          )}
        </div>
      </div>
    </>
  );
}

function RenderLeaderSchedule({
  leaderSchedule,
}: {
  leaderSchedule: LeaderSchedule;
}): JSX.Element {
  const [displayedCount, setDisplayedCount] =
    React.useState<number>(PAGINATION_COUNT);
  return (
    <>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h2 className="card-header-title">
                These are the Leader Schedule for the current epoch
              </h2>
            </div>
            <div className="col-auto">
              Total Nodes: {Object.keys(leaderSchedule).length}
            </div>
          </div>
        </div>
        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="text-muted">#</th>
                <th className="text-muted">IDENTITY</th>
              </tr>
            </thead>
            {Object.keys(leaderSchedule)
              .slice(0, displayedCount)
              .map((leaderPubKey: string, index: number) => {
                const data = {
                  leaderKey: leaderPubKey,
                  data: leaderSchedule[leaderPubKey],
                };
                return renderLeaderSchedule(index, data);
              })}
          </table>
          {Object.keys(leaderSchedule).length > PAGINATION_COUNT && (
            <div className="card-footer">
              <button
                className="btn btn-primary w-100"
                onClick={() =>
                  setDisplayedCount(
                    (displayed: number): number => displayed + PAGINATION_COUNT
                  )
                }
              >
                Load More
              </button>
            </div>
          )}
        </div>
      </div>
    </>
  );
}

type leaderSchedule = {
  leaderKey: string;
  data: number[];
};
function renderLeaderSchedule(
  index: number,
  data: leaderSchedule
): JSX.Element {
  const leaderKey: PublicKey = new PublicKey(data.leaderKey);

  return (
    <tr key={index}>
      <td>
        <span className="badge bg-gray-soft badge-pill">{index + 1}</span>
      </td>
      <td className="text-start">
        <Address pubkey={leaderKey} link />
      </td>
    </tr>
  );
}

function renderRowData(index: number, data: rowData): JSX.Element {
  const validatorPublicKey: PublicKey = new PublicKey(data.nodePubkey);
  const votePublicKey: PublicKey = new PublicKey(data.votePubkey);
  return (
    <tr key={index}>
      <td>
        <span className="badge bg-gray-soft badge-pill">{index + 1}</span>
      </td>
      <td className="text-start">
        <Address pubkey={validatorPublicKey} link />
      </td>
      <td className="text-end">{data.activatedStake}</td>
      <td className="text-end">{data.commission}</td>
      <td className="text-end">{data.lastVote}</td>
      <td className="text-start">
        <Address pubkey={votePublicKey} link />
      </td>
    </tr>
  );
}
