import React from "react";
import { Address } from "components/common/Address";
import { camelToTitleCase } from "utils";
import { mapIxArgsToRows } from "providers/idl/display";
import { InstructionParsed } from "providers/idl/parsing";
import { Idl } from "@project-serum/anchor";

export function RawIdlParsedDetails({
  ixParsed,
  idl,
}: {
  ixParsed: InstructionParsed;
  idl: Idl;
}) {
  return (
    <>
      <table className="table table-sm table-nowrap card-table">
        <tbody className="list">
          <tr className="table-sep">
            <td>Account Name</td>
            <td className="text-lg-end">Address</td>
          </tr>
          {ixParsed.accounts.map((item, keyIndex) => {
            if ("pubkey" in item) {
              const { pubkey, isSigner, isWritable, name } = item;
              return (
                <tr key={keyIndex}>
                  <td>
                    <div className="me-2 d-md-inline">
                      {camelToTitleCase(name)}
                    </div>
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
                  <td className="text-lg-end">
                    {accounts.length} nested accounts
                  </td>
                </tr>
              );
            }
          })}
        </tbody>
      </table>
      <table className="table table-sm table-nowrap card-table">
        <tbody className="list">
          <tr className="table-sep">
            <td>Argument Name</td>
            <td>Type</td>
            <td className="text-lg-end">Value</td>
          </tr>
          {mapIxArgsToRows(ixParsed.data, ixParsed.type, idl)}
        </tbody>
      </table>
    </>
  );
}
