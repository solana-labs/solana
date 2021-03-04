import React from "react";
import { TableCardBody } from "components/common/TableCardBody";
import { lamportsToSolString } from "utils";
import { Account, useFetchAccountInfo } from "providers/accounts";
import { Address } from "components/common/Address";
import {
  ProgramAccountInfo,
  ProgramDataAccountInfo,
} from "validators/accounts/upgradeable-program";
import { Slot } from "components/common/Slot";
import { addressLabel } from "utils/tx";
import { useCluster } from "providers/cluster";

export function UpgradeableProgramSection({
  account,
  programAccount,
  programData,
}: {
  account: Account;
  programAccount: ProgramAccountInfo;
  programData: ProgramDataAccountInfo;
}) {
  const refresh = useFetchAccountInfo();
  const { cluster } = useCluster();
  const label = addressLabel(account.pubkey.toBase58(), cluster);
  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Program Account
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
            <Address pubkey={account.pubkey} alignRight raw />
          </td>
        </tr>
        {label && (
          <tr>
            <td>Address Label</td>
            <td className="text-lg-right">{label}</td>
          </tr>
        )}
        <tr>
          <td>Balance (SOL)</td>
          <td className="text-lg-right text-uppercase">
            {lamportsToSolString(account.lamports || 0)}
          </td>
        </tr>
        <tr>
          <td>Executable</td>
          <td className="text-lg-right">Yes</td>
        </tr>
        <tr>
          <td>Executable Data</td>
          <td className="text-lg-right">
            <Address pubkey={programAccount.programData} alignRight link />
          </td>
        </tr>
        <tr>
          <td>Upgradeable</td>
          <td className="text-lg-right">
            {programData.authority !== null ? "Yes" : "No"}
          </td>
        </tr>
        <tr>
          <td>Last Deployed Slot</td>
          <td className="text-lg-right">
            <Slot slot={programData.slot} link />
          </td>
        </tr>
        {programData.authority !== null && (
          <tr>
            <td>Upgrade Authority</td>
            <td className="text-lg-right">
              <Address pubkey={programData.authority} alignRight link />
            </td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}
