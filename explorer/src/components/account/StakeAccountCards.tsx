import React from "react";
import { StakeAccount, Meta } from "solana-sdk-wasm";
import TableCardBody from "components/common/TableCardBody";
import { lamportsToSolString } from "utils";
import { displayTimestamp } from "utils/date";
import { Account, useFetchAccountInfo } from "providers/accounts";
import Address from "components/common/Address";

export function StakeAccountCards({
  account,
  stakeAccount,
}: {
  account: Account;
  stakeAccount: StakeAccount;
}) {
  return (
    <>
      <LockupCard stakeAccount={stakeAccount} />
      <OverviewCard account={account} stakeAccount={stakeAccount} />
      {stakeAccount.meta && <DelegationCard stakeAccount={stakeAccount} />}
      {stakeAccount.meta && <AuthoritiesCard meta={stakeAccount.meta} />}
    </>
  );
}

function LockupCard({ stakeAccount }: { stakeAccount: StakeAccount }) {
  const unixTimestamp = stakeAccount.meta?.lockup.unixTimestamp;
  if (unixTimestamp && unixTimestamp > 0) {
    const prettyTimestamp = displayTimestamp(unixTimestamp * 1000);
    return (
      <div className="alert alert-warning text-center">
        <strong>Account is locked!</strong> Lockup expires on {prettyTimestamp}
      </div>
    );
  } else {
    return null;
  }
}

function OverviewCard({
  account,
  stakeAccount,
}: {
  account: Account;
  stakeAccount: StakeAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Stake Account
        </h3>
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
            <Address pubkey={account.pubkey} alignRight />
          </td>
        </tr>
        <tr>
          <td>Balance (SOL)</td>
          <td className="text-lg-right text-uppercase">
            {lamportsToSolString(account.lamports || 0)}
          </td>
        </tr>
        {stakeAccount.meta && (
          <tr>
            <td>Rent Reserve (SOL)</td>
            <td className="text-lg-right">
              {lamportsToSolString(stakeAccount.meta.rentExemptReserve)}
            </td>
          </tr>
        )}
        {!stakeAccount.meta && (
          <tr>
            <td>State</td>
            <td className="text-lg-right">{stakeAccount.displayState()}</td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}

function DelegationCard({ stakeAccount }: { stakeAccount: StakeAccount }) {
  const { stake } = stakeAccount;
  const displayStatus = () => {
    let status = stakeAccount.displayState();
    if (status !== "Delegated") {
      status = "Not delegated";
    }
    return status;
  };

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Stake Delegation
        </h3>
      </div>
      <TableCardBody>
        <tr>
          <td>Status</td>
          <td className="text-lg-right">{displayStatus()}</td>
        </tr>

        {stake && (
          <>
            <tr>
              <td>Delegated Stake (SOL)</td>
              <td className="text-lg-right">
                {lamportsToSolString(stake.delegation.stake)}
              </td>
            </tr>

            <tr>
              <td>Delegated Vote Address</td>
              <td className="text-lg-right">
                <Address
                  pubkey={stake.delegation.voterPubkey}
                  alignRight
                  link
                />
              </td>
            </tr>

            <tr>
              <td>Activation Epoch</td>
              <td className="text-lg-right">
                {stake.delegation.isBootstrapStake()
                  ? "-"
                  : stake.delegation.activationEpoch}
              </td>
            </tr>

            <tr>
              <td>Deactivation Epoch</td>
              <td className="text-lg-right">
                {stake.delegation.isDeactivated()
                  ? stake.delegation.deactivationEpoch
                  : "-"}
              </td>
            </tr>
          </>
        )}
      </TableCardBody>
    </div>
  );
}

function AuthoritiesCard({ meta }: { meta: Meta }) {
  const hasLockup = meta && meta.lockup.unixTimestamp > 0;
  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Authorities
        </h3>
      </div>
      <TableCardBody>
        <tr>
          <td>Stake Authority Address</td>
          <td className="text-lg-right">
            <Address pubkey={meta.authorized.staker} alignRight link />
          </td>
        </tr>

        <tr>
          <td>Withdraw Authority Address</td>
          <td className="text-lg-right">
            <Address pubkey={meta.authorized.withdrawer} alignRight link />
          </td>
        </tr>

        {hasLockup && (
          <tr>
            <td>Lockup Authority Address</td>
            <td className="text-lg-right">
              <Address pubkey={meta.lockup.custodian} alignRight link />
            </td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}
