import React from "react";
import { Account, useFetchAccountInfo } from "providers/accounts";
import { TableCardBody } from "components/common/TableCardBody";
import { Address } from "components/common/Address";
import { VoteAccount } from "validators/accounts/vote";
import { displayTimestamp } from "utils/date";
import {
  AccountHeader,
  AccountAddressRow,
  AccountBalanceRow,
} from "components/common/Account";

export function VoteAccountSection({
  account,
  voteAccount,
}: {
  account: Account;
  voteAccount: VoteAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader
        title="Vote Account"
        refresh={() => refresh(account.pubkey)}
      />

      <TableCardBody>
        <AccountAddressRow account={account} />
        <AccountBalanceRow account={account} />

        <tr>
          <td>
            Authorized Voter
            {voteAccount.info.authorizedVoters.length > 1 ? "s" : ""}
          </td>
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
          <td>Last Timestamp</td>
          <td className="text-lg-right">
            {displayTimestamp(voteAccount.info.lastTimestamp.timestamp * 1000)}
          </td>
        </tr>

        <tr>
          <td>Commission</td>
          <td className="text-lg-right">{voteAccount.info.commission + "%"}</td>
        </tr>

        <tr>
          <td>Root Slot</td>
          <td className="text-lg-right">{voteAccount.info.rootSlot}</td>
        </tr>
      </TableCardBody>
    </div>
  );
}
