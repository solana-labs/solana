import React from "react";
import { Account } from "providers/accounts";
import { lamportsToSolString } from "utils";
import { TableCardBody } from "components/common/TableCardBody";
import { Address } from "components/common/Address";

export function UnknownAccountCard({ account }: { account: Account }) {
  const { details, lamports } = account;
  if (lamports === undefined) return null;

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
        <tr>
          <td>Balance (SOL)</td>
          <td className="text-lg-right text-uppercase">
            {lamportsToSolString(lamports)}
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
