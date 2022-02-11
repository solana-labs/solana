import React from "react";
import { Address } from "components/common/Address";
import { HexData } from "components/common/HexData";
import { TransactionInstructionIdlParsed } from "utils/instruction";
import { camelToTitleCase } from "utils";

export function RawIdlParsedDetails({
  ixParsed,
}: {
  ixParsed: TransactionInstructionIdlParsed;
}) {
  console.log("huh");

  return (
    <>
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

      <tr>
        <td>
          Instruction Data <span className="text-muted">(Hex)</span>
        </td>
        <td className="text-lg-end">
          <HexData raw={ixParsed.data} />
        </td>
      </tr>
    </>
  );
}
