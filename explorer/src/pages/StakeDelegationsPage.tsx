import React, { useEffect } from "react";

import {
  useVoteAccounts,
  Status,
  useFetchVoteAccounts,
} from "providers/accounts/vote-accounts";
import { ClusterStatus, useCluster } from "providers/cluster";
import { LoadingCard } from "components/common/LoadingCard";
import { Address } from "components/common/Address";
import { LeaderSchedule, PublicKey, VoteAccountStatus } from "@solana/web3.js";

import {
  useFetchLeaderSchedule,
  useLeaderSchedule,
} from "providers/accounts/leader-schedule";
import MapChart from "components/WorldMap";
import { ErrorCard } from "components/common/ErrorCard";

const PAGINATION_COUNT: number = 7;

export function StakeDelegationsPage() {
  return (
    <>
      <div>
        <MapChart />
        <RenderVoteAccount />
        <RenderLeaderSchedule />
      </div>
    </>
  );
}

function RenderVoteAccount() {
  const voteAccounts = useVoteAccounts();
  const fetchVoteAccounts = useFetchVoteAccounts();

  React.useEffect(() => {
    if (voteAccounts === Status.Idle) fetchVoteAccounts();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  if (voteAccounts === Status.Disconnected) {
    return <ErrorCard text="Not connected to the cluster" />;
  }

  if (voteAccounts === Status.Idle || voteAccounts === Status.Connecting)
    return <LoadingCard />;

  if (typeof voteAccounts === "string") {
    return <ErrorCard text={voteAccounts} retry={fetchVoteAccounts} />;
  }

  return (
    <React.Fragment>
      <RenderCardSection
        title="Current Validators Data"
        cardData={voteAccounts.current}
      />
      <RenderCardSection
        title="Deliquent Validators Data"
        cardData={voteAccounts.delinquent}
      />
    </React.Fragment>
  );
}

interface rowData {
  activatedStake: number;
  commission: number;
  lastVote: number;
  nodePubkey: string;
  votePubkey: string;
}
//will need to modify card data
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
      <div className="container card mt-n3 mb-6">
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
                <th className="text-muted ">ACTIVE STAKE</th>
                <th className="text-muted ">COMMISSION</th>
                <th className="text-muted ">LAST VOTE</th>
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

function RenderLeaderSchedule(): JSX.Element {
  const leaderSchedule = useLeaderSchedule();
  const fetchLeaderSchedule = useFetchLeaderSchedule();

  const [displayedCount, setDisplayedCount] =
    React.useState<number>(PAGINATION_COUNT);

  React.useEffect(() => {
    if (leaderSchedule === Status.Idle) fetchLeaderSchedule();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  if (leaderSchedule === Status.Disconnected) {
    return <ErrorCard text="Not connected to the cluster" />;
  }

  if (leaderSchedule === Status.Idle || leaderSchedule === Status.Connecting)
    return <LoadingCard />;

  if (typeof leaderSchedule === "string") {
    return <ErrorCard text={leaderSchedule} retry={fetchLeaderSchedule} />;
  }

  return (
    <>
      <div className="container card mt-n3">
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
        <div className="table-responsive mb-100">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="text-muted">#</th>
                <th className="text-muted">IDENTITY</th>
              </tr>
            </thead>
            <tbody className="list">
              {Object.keys(leaderSchedule)
                .slice(0, displayedCount)
                .map((leaderPubKey: string, index: number): JSX.Element => {
                  const data = {
                    leaderKey: leaderPubKey,
                    data: leaderSchedule[leaderPubKey],
                  };
                  return renderLeaderSchedule(index, data);
                })}
            </tbody>
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
