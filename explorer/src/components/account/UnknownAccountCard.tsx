import React from "react";
import { Account } from "providers/accounts";
import { lamportsToSafeString } from "utils";
import { TableCardBody } from "components/common/TableCardBody";
import { Address } from "components/common/Address";
import { addressLabel } from "utils/tx";
import { useCluster } from "providers/cluster";

export function UnknownAccountCard({ account }: { account: Account }) {
  const { details, lamports } = account;
  const { cluster } = useCluster();
  if (lamports === undefined) return null;

  const label = addressLabel(account.pubkey.toBase58(), cluster);
  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Overview</h3>
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
          <td>Balance (SAFE)</td>
          <td className="text-lg-right text-uppercase">
            {lamportsToSafeString(lamports)}
          </td>
        </tr>

        {details?.space !== undefined && (
          <tr>
            <td>Data (Bytes)</td>
            <td className="text-lg-right">{details.space}</td>
          </tr>
        )}

        {details && (
          <tr>
            <td>Owner</td>
            <td className="text-lg-right">
              <Address pubkey={details.owner} alignRight link />
            </td>
          </tr>
        )}

        {details && (
          <tr>
            <td>Executable</td>
            <td className="text-lg-right">
              {details.executable ? "Yes" : "No"}
            </td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}
