import React from "react";
import { Address } from "components/common/Address";
import { TransactionInstructionIdlParsed } from "utils/instruction";
import { camelToTitleCase } from "utils";
import { mapDataObjectToRows } from "providers/accounts/idl";

export function RawIdlParsedDetails({
  ixParsed,
}: {
  ixParsed: TransactionInstructionIdlParsed;
}) {
  return (
    <>
      <tr className="table-sep">
        <td colSpan={2}>Instruction Accounts</td>
      </tr>
      {ixParsed.accounts.map((item, keyIndex) => {
        if ("pubkey" in item) {
          const { pubkey, isSigner, isWritable, name } = item;
          return (
            <tr key={keyIndex}>
              <td>
                <div className="me-2 d-md-inline">{camelToTitleCase(name)}</div>
                {isWritable && (
                  <span className="badge bg-info-soft me-1">Writable</span>
                )}
                {isSigner && (
                  <span className="badge bg-info-soft me-1">Signer</span>
                )}
              </td>
              <td className="text-lg-end">
                <Address pubkey={pubkey} alignRight link />
              </td>
            </tr>
          );
        } else {
          const { name, accounts } = item;
          return (
            <tr key={keyIndex}>
              <td>
                <div className="me-2 d-md-inline">{name}</div>
              </td>
              <td className="text-lg-end">{accounts.length} nested accounts</td>
            </tr>
          );
        }
      })}

      <tr className="table-sep">
        <td colSpan={2}>Instruction Data</td>
      </tr>
      {mapDataObjectToRows(ixParsed.data.parsed)}
    </>
  );
}
