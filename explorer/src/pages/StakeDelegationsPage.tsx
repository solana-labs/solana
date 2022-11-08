import { useEffect } from "react";

import { useVoteAccounts } from "providers/accounts/vote-accounts";
import { ClusterStatus, useCluster } from "providers/cluster";
import { LoadingCard } from "components/common/LoadingCard";
import { Address } from "components/common/Address";
import { PublicKey, VoteAccountInfo } from "@solana/web3.js";

export function StakeDelegationsPage() {
  const { status } = useCluster();

  const { voteAccounts, fetchVoteAccounts } = useVoteAccounts();
  useEffect(() => {
    if (status === ClusterStatus.Connected) fetchVoteAccounts();
  }, [status]);
  console.log(voteAccounts);
  if (voteAccounts)
    return (
      <>
        <div className="card">
          <div className="card-header">
            <div className="row align-items-center">
              <div className="col">
                <h2 className="card-header-title"> Current Validator Data </h2>
              </div>
              <div className="col-auto">
                Total Nodes: {voteAccounts?.current.length}
              </div>
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
                {voteAccounts.current.map((data: any, index: number) => {
                  return renderRowData(index, data);
                })}
              </tbody>
            </table>
          </div>
        </div>
        <div className="card">
          <div className="card-header">
            <div className="row align-items-center">
              <div className="col">
                <h2 className="card-header-title centered ">Network Nodes</h2>
              </div>
              <div className="col-auto">Total Nodes:34</div>
            </div>
          </div>
          <div className="table-responsive mb-0">
            <table className="table table-sm table-nowrap card-table">
              <thead>
                <tr>
                  <th className="text-muted">#</th>
                  <th className="text-muted">IDENTITY</th>
                  <th className="text-muted text-end">IP</th>
                  <th className="text-muted text-end">Gossip</th>
                  <th className="text-muted text-end">TPU</th>
                  <th className="text-muted text-end">Version</th>
                </tr>
              </thead>
              <tbody className="list">
                <tr>
                  <td>
                    <span className="badge bg-gray-soft badge-pill">
                      goog thing
                    </span>
                  </td>
                  <td className="text-start">3439894343</td>
                  <td className="text-end">good thing</td>
                  <td className="text-end">NUll page</td>
                  <td className="text-end">googd</td>
                  <td className="text-end">dfdtere</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </>

      // <div className="container mt-4">
      //   <StakingComponent />
      //   <div className="card">
      //     <div className="card-header">
      //       <div className="row align-items-center">
      //         <div className="col">
      //           <h4 className="card-header-title">
      //             Staking Data for the Network
      //           </h4>
      //         </div>
      //       </div>
      //     </div>
      //   </div>
      // </div>
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
