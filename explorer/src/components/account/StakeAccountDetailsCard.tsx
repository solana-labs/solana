import React from "react";
import { StakeAccount } from "solana-sdk-wasm";
import TableCardBody from "components/common/TableCardBody";
import { lamportsToSolString } from "utils";
import Copyable from "components/Copyable";
import { displayAddress } from "utils/tx";

export function StakeAccountDetailsCard({
  account
}: {
  account: StakeAccount;
}) {
  const { meta, stake } = account;
  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Stake Account
        </h3>
      </div>
      <TableCardBody>
        <tr>
          <td>State</td>
          <td className="text-right">{account.displayState()}</td>
        </tr>

        {meta && (
          <>
            <tr>
              <td>Rent Reserve (SOL)</td>
              <td className="text-right">
                {lamportsToSolString(meta.rentExemptReserve)}
              </td>
            </tr>

            <tr>
              <td>Authorized Staker Address</td>
              <td className="text-right">
                <Copyable text={meta.authorized.staker.toBase58()}>
                  <code>{meta.authorized.staker.toBase58()}</code>
                </Copyable>
              </td>
            </tr>

            <tr>
              <td>Authorized Withdrawer Address</td>
              <td className="text-right">
                <Copyable text={meta.authorized.withdrawer.toBase58()}>
                  <code>{meta.authorized.withdrawer.toBase58()}</code>
                </Copyable>
              </td>
            </tr>

            <tr>
              <td>Lockup Expiry Epoch</td>
              <td className="text-right">{meta.lockup.epoch}</td>
            </tr>

            <tr>
              <td>Lockup Expiry Timestamp</td>
              <td className="text-right">
                {new Date(meta.lockup.unixTimestamp).toUTCString()}
              </td>
            </tr>

            <tr>
              <td>Lockup Custodian Address</td>
              <td className="text-right">
                <Copyable text={meta.lockup.custodian.toBase58()}>
                  <code>
                    {displayAddress(meta.lockup.custodian.toBase58())}
                  </code>
                </Copyable>
              </td>
            </tr>
          </>
        )}

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
