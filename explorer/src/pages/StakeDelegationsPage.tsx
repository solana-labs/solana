import { useEffect } from "react";

import { useVoteAccounts } from "providers/accounts/vote-accounts";

export function StakeDelegationsPage() {
  const { voteAccounts, fetchVoteAccounts } = useVoteAccounts();
  useEffect(() => {
    fetchVoteAccounts();
    console.log(voteAccounts?.current);
  }, [voteAccounts]);

  // return <div> For testing purposes</div>;
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
