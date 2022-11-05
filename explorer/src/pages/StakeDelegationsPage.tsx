import React from "react";

import axios from "axios";
import { TableCardBody } from "components/common/TableCardBody";
import { Slot } from "components/common/Slot";

import {
  ClusterStatsStatus,
  useDashboardInfo,
  usePerformanceInfo,
  useStatsProvider,
} from "providers/stats/solanaClusterStats";
import { abbreviatedNumber, lamportsToSol, slotsToHumanString } from "utils";
import { ClusterStatus, useCluster } from "providers/cluster";
import { LiveTransactionStatsCard } from "components/LiveTransactionStatsCard";
import { displayTimestampWithoutDate } from "utils/date";
import { Status, useFetchSupply, useSupply } from "providers/supply";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { useVoteAccounts } from "providers/accounts/vote-accounts";
import { CoingeckoStatus, useCoinGecko } from "utils/coingecko";
import { Epoch } from "components/common/Epoch";
import { TimestampToggle } from "components/common/TimestampToggle";
import { StakeMeta } from "validators/accounts/stake";

const CLUSTER_STATS_TIMEOUT = 5000;

export function StakeDelegationsPage() {
  const url = "https://api.solanabeach.io/v1";
  const auth = "d2498ef9-f0c7-4551-9f9f-69b26366204f";

  const config = {
    headers: { Authorization: `Bearer ${auth}` },
  };

  const bodyParameters = {
    key: "value",
  };

  axios.post(url, bodyParameters, config).then(console.log).catch(console.log);
  return (
    <div className="container mt-4">
      <StakingComponent />
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h4 className="card-header-title">
                Staking Data for the Network
              </h4>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function StakingComponent() {
  return (
    <div className="row staking-card">
      <div className="col-12 col-lg-4 col-xl">
        <div className="card">
          <div className="card-body">
            <h4>Circulating Supply</h4>
            <h1>Fair</h1>
            <h5>
              <em>10 %</em> is circulating
            </h5>
          </div>
        </div>
      </div>

      <div className="card">
        <h1 className="card-header-title  ml-2 mt-2 mb-3">Delegators</h1>
        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="text-muted c-pointer"> </th>
                <th className="text-muted">Result</th>
                <th className="text-muted">Transaction Signature</th>
              </tr>
            </thead>
            <tbody className="list">
              <tr key={1}>
                <td>{1}</td>
                <td>
                  <span className={`badge bg-success-soft`}> success</span>
                </td>

                <td>{1234}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <div className="card">
        <h1 className="card-header-title  ml-2 mt-2 mb-3">Validators</h1>
        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="text-muted c-pointer">Url</th>
                <th className="text-muted">Amount</th>
                <th className="text-muted">Transaction Cost</th>
              </tr>
            </thead>
            <tbody className="list">
              <tr key={1}>
                <td>{1}</td>
                <td>
                  <span className={`badge bg-success-soft`}> success</span>
                </td>

                <td>{1234}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
