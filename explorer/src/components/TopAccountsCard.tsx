import React from "react";
import { Link } from "react-router-dom";
import { AccountBalancePair } from "@solana/web3.js";
import Copyable from "./Copyable";
import { useRichList, useFetchRichList, Status } from "providers/richList";
import LoadingCard from "./common/LoadingCard";
import ErrorCard from "./common/ErrorCard";
import { lamportsToSolString } from "utils";

export default function TopAccountsCard() {
  const richList = useRichList();
  const fetchRichList = useFetchRichList();

  // Fetch on load
  React.useEffect(() => {
    fetchRichList();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  if (richList === Status.Disconnected) {
    return <ErrorCard text="Not connected to the cluster" />;
  }

  if (richList === Status.Idle || richList === Status.Connecting)
    return <LoadingCard />;

  if (typeof richList === "string") {
    return <ErrorCard text={richList} retry={fetchRichList} />;
  }

  const { accounts, circulatingSupply: supply } = richList;

  return (
    <div className="card">
      {renderHeader()}

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">Rank</th>
              <th className="text-muted">Address</th>
              <th className="text-muted">Balance (SOL)</th>
              <th className="text-muted">% of Circulating Supply</th>
              <th className="text-muted">Details</th>
            </tr>
          </thead>
          <tbody className="list">
            {accounts.map((account, index) =>
              renderAccountRow(account, index, supply)
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

const renderHeader = () => {
  return (
    <div className="card-header">
      <div className="row align-items-center">
        <div className="col">
          <h4 className="card-header-title">Top 20 Active Accounts</h4>
        </div>
      </div>
    </div>
  );
};

const renderAccountRow = (
  account: AccountBalancePair,
  index: number,
  supply: number
) => {
  const base58AccountPubkey = account.address.toBase58();
  return (
    <tr key={index}>
      <td>
        <span className="badge badge-soft-dark badge-pill">{index + 1}</span>
      </td>
      <td>
        <Copyable text={base58AccountPubkey}>
          <code>{base58AccountPubkey}</code>
        </Copyable>
      </td>
      <td>{lamportsToSolString(account.lamports, 0)}</td>
      <td>{`${((100 * account.lamports) / supply).toFixed(3)}%`}</td>
      <td>
        <Link
          to={location => ({
            ...location,
            pathname: "/account/" + base58AccountPubkey
          })}
          className="btn btn-rounded-circle btn-white btn-sm"
        >
          <span className="fe fe-arrow-right"></span>
        </Link>
      </td>
    </tr>
  );
};
