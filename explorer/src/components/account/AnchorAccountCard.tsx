import React, { useMemo } from "react";
import { Account } from "providers/accounts";
import { useCluster } from "providers/cluster";
import { BorshAccountsCoder } from "@project-serum/anchor";
import { IdlTypeDef } from "@project-serum/anchor/dist/cjs/idl";
import { getAnchorProgramName, mapAccountToRows } from "utils/anchor";
import { ErrorCard } from "components/common/ErrorCard";
import { useAnchorProgram } from "providers/anchor";

export function AnchorAccountCard({ account }: { account: Account }) {
  const { lamports } = account;
  const { url } = useCluster();
  const anchorProgram = useAnchorProgram(
    account.details?.owner.toString() || "",
    url
  );
  const rawData = account?.details?.rawData;
  const programName = getAnchorProgramName(anchorProgram) || "Unknown Program";

  const { decodedAccountData, accountDef } = useMemo(() => {
    let decodedAccountData: any | null = null;
    let accountDef: IdlTypeDef | undefined = undefined;
    if (anchorProgram && rawData) {
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
  if (!anchorProgram) return <ErrorCard text="No Anchor IDL found" />;
  if (!decodedAccountData || !accountDef) {
    return (
      <ErrorCard text="Failed to decode account data according to the public Anchor interface" />
    );
  }

  return (
    <div>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h3 className="card-header-title">
                {programName}: {accountDef.name}
              </h3>
            </div>
          </div>
        </div>

        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="w-1">Field</th>
                <th className="w-1">Type</th>
                <th className="w-1">Value</th>
              </tr>
            </thead>
            <tbody>
              {mapAccountToRows(
                decodedAccountData,
                accountDef as IdlTypeDef,
                anchorProgram.idl
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
