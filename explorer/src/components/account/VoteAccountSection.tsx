import React from "react";

import { Account, useFetchAccountInfo } from "providers/accounts";

import { TableCardBody } from "components/common/TableCardBody";
import { Address } from "components/common/Address";
import { VoteAccount } from "validators/accounts/vote";

export function VoteAccountSection({
  account,
  voteAccount,
}: {
  account: Account;
  voteAccount: VoteAccount;
}) {
  return <VoteAccountCard account={account} voteAccount={voteAccount} />;
}

function VoteAccountCard({
  account,
  voteAccount,
}: {
  account: Account;
  voteAccount: VoteAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Vote Account</h3>
        <button
          className="btn btn-white btn-sm"
          onClick={() => refresh(account.pubkey)}
        >
          <span className="fe fe-refresh-cw mr-2"></span>
          Refresh
        </button>
      </div>
      <TableCardBody>
        <tr>
          <td>Address</td>
          <td className="text-lg-right">
            <Address pubkey={account.pubkey} alignRight raw />
          </td>
        </tr>
        <tr>
          <td>Authorized Voters</td>
          <td className="text-lg-right">
            {voteAccount.info.authorizedVoters.map((voter) => {
              return (
                <Address
                  pubkey={voter.authorizedVoter}
                  key={voter.authorizedVoter.toString()}
                  alignRight
                  raw
                  link
                />
              );
            })}
          </td>
        </tr>
        <tr>
          <td>Authorized Withdrawer</td>
          <td className="text-lg-right">
            <Address
              pubkey={voteAccount.info.authorizedWithdrawer}
              alignRight
              raw
              link
            />
          </td>
        </tr>
        <tr>
          <td>Commission</td>
          <td className="text-lg-right">{voteAccount.info.commission + "%"}</td>
        </tr>
      </TableCardBody>
    </div>
  );
}
