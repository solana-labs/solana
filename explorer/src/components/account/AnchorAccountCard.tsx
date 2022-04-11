import React, { useMemo } from "react";
import { Account } from "providers/accounts";
import { SolBalance } from "utils";
import { TableCardBody } from "components/common/TableCardBody";
import { Address } from "components/common/Address";
import { addressLabel } from "utils/tx";
import { useCluster } from "providers/cluster";
import { useTokenRegistry } from "providers/mints/token-registry";
import { BorshAccountsCoder, Program } from "@project-serum/anchor";
import { IdlTypeDef } from "@project-serum/anchor/dist/cjs/idl";
import { mapAccountToRows } from "utils/anchor";
import { ErrorCard } from "components/common/ErrorCard";

export function AnchorAccountCard({
  account,
  rawData,
  anchorProgram,
}: {
  account: Account;
  rawData: Buffer;
  anchorProgram: Program;
}) {
  const { details, lamports } = account;
  const { cluster } = useCluster();
  const { tokenRegistry } = useTokenRegistry();

  const { decodedAccountData, accountDef } = useMemo(() => {
    let decodedAccountData: any | null = null;
    let accountDef: IdlTypeDef | undefined = undefined;
    if (anchorProgram) {
      const coder = new BorshAccountsCoder(anchorProgram.idl);
      const accountDefTmp = anchorProgram.idl.accounts?.find(
        (accountType: any) =>
          (rawData as Buffer)
            .slice(0, 8)
            .equals(BorshAccountsCoder.accountDiscriminator(accountType.name))
      );
      if (accountDefTmp) {
        accountDef = accountDefTmp;
        decodedAccountData = coder.decode(accountDef.name, rawData);
      }
    }

    return {
      decodedAccountData,
      accountDef,
    };
  }, [anchorProgram, rawData]);

  if (lamports === undefined) return null;

  if (!decodedAccountData || !accountDef) {
    return (
      <ErrorCard text="Failed to decode account data according to its public anchor interface" />
    );
  }

  const label = addressLabel(account.pubkey.toBase58(), cluster, tokenRegistry);
  return (
    <div>
      <div className="card">
        <div className="card-header align-items-center">
          <h3 className="card-header-title">Overview</h3>
        </div>

        <TableCardBody>
          <tr>
            <td>Address</td>
            <td className="text-lg-end">
              <Address pubkey={account.pubkey} alignRight link />
            </td>
          </tr>
          {label && (
            <tr>
              <td>Address Label</td>
              <td className="text-lg-end">{label}</td>
            </tr>
          )}
          <tr>
            <td>Balance (SOL)</td>
            <td className="text-lg-end">
              <SolBalance lamports={lamports} />
            </td>
          </tr>

          {details?.space !== undefined && (
            <tr>
              <td>Allocated Data Size</td>
              <td className="text-lg-end">{details.space} byte(s)</td>
            </tr>
          )}

          {details && (
            <tr>
              <td>Assigned Program Id</td>
              <td className="text-lg-end">
                <Address pubkey={details.owner} alignRight link />
              </td>
            </tr>
          )}

          {details && (
            <tr>
              <td>Executable</td>
              <td className="text-lg-end">
                {details.executable ? "Yes" : "No"}
              </td>
            </tr>
          )}
        </TableCardBody>
      </div>
      {accountDef?.name && (
        <div className="card">
          <div className="card-header align-items-center">
            <h3 className="card-header-title">
              Data - {accountDef.name} (Anchor)
            </h3>
          </div>

          <TableCardBody>
            <tr className="table-sep">
              <td className="text-muted w-1">Field</td>
              <td>Type</td>
              <td className="text-lg-end">Value</td>
            </tr>
            {mapAccountToRows(
              decodedAccountData,
              accountDef as IdlTypeDef,
              anchorProgram.idl
            )}
          </TableCardBody>
        </div>
      )}
    </div>
  );
}
