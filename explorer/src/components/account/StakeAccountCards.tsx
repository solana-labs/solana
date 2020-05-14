import React from "react";
import { StakeAccount, Meta } from "solana-sdk-wasm";
import TableCardBody from "components/common/TableCardBody";
import { lamportsToSolString } from "utils";
import Copyable from "components/Copyable";
import { displayAddress } from "utils/tx";
import { Account, useFetchAccountInfo } from "providers/accounts";

export function StakeAccountCards({
  account,
  stakeAccount
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
    const expireDate = new Date(unixTimestamp * 1000);
    return (
      <div className="alert alert-warning text-center">
        <strong>Account is locked!</strong> Lockup expires on{" "}
        {expireDate.toLocaleDateString()} at {expireDate.toLocaleTimeString()}
      </div>
    );
  } else {
    return null;
  }
}

function OverviewCard({
  account,
  stakeAccount
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
          <td className="text-right">
            <Copyable text={account.pubkey.toBase58()}>
              <code>{account.pubkey.toBase58()}</code>
            </Copyable>
          </td>
        </tr>
        <tr>
          <td>Balance (SOL)</td>
          <td className="text-right text-uppercase">
            {lamportsToSolString(account.lamports || 0)}
          </td>
        </tr>
        {stakeAccount.meta && (
          <tr>
            <td>Rent Reserve (SOL)</td>
            <td className="text-right">
              {lamportsToSolString(stakeAccount.meta.rentExemptReserve)}
            </td>
          </tr>
        )}
        {!stakeAccount.meta && (
          <tr>
            <td>State</td>
            <td className="text-right">{stakeAccount.displayState()}</td>
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
          <td className="text-right">{displayStatus()}</td>
        </tr>

        {stake && (
          <>
            <tr>
              <td>Delegated Stake (SOL)</td>
              <td className="text-right">
                {lamportsToSolString(stake.delegation.stake)}
              </td>
            </tr>

            <tr>
              <td>Delegated Vote Address</td>
              <td className="text-right">
                <Copyable text={stake.delegation.voterPubkey.toBase58()}>
                  <code>
                    {displayAddress(stake.delegation.voterPubkey.toBase58())}
                  </code>
                </Copyable>
              </td>
            </tr>

            <tr>
              <td>Activation Epoch</td>
              <td className="text-right">
                {stake.delegation.isBootstrapStake()
                  ? "-"
                  : stake.delegation.activationEpoch}
              </td>
            </tr>

            <tr>
              <td>Deactivation Epoch</td>
              <td className="text-right">
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
          <td className="text-right">
            <Copyable text={meta.authorized.staker.toBase58()}>
              <code>{meta.authorized.staker.toBase58()}</code>
            </Copyable>
          </td>
        </tr>

        <tr>
          <td>Withdraw Authority Address</td>
          <td className="text-right">
            <Copyable text={meta.authorized.withdrawer.toBase58()}>
              <code>{meta.authorized.withdrawer.toBase58()}</code>
            </Copyable>
          </td>
        </tr>

        {hasLockup && (
          <tr>
            <td>Lockup Authority Address</td>
            <td className="text-right">
              <Copyable text={meta.lockup.custodian.toBase58()}>
                <code>{displayAddress(meta.lockup.custodian.toBase58())}</code>
              </Copyable>
            </td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}
