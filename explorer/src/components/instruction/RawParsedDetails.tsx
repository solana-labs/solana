import React from "react";
import { ParsedInstruction } from "@solana/web3.js";

export function RawParsedDetails({
  ix,
  children,
}: {
  ix: ParsedInstruction;
  children?: React.ReactNode;
}) {
  return (
    <>
      {children}

      <tr>
        <td>
          Instruction Data <span className="text-muted">(JSON)</span>
        </td>
        <td className="text-lg-right">
          <pre className="d-inline-block text-left json-wrap">
            {JSON.stringify(ix.parsed, null, 2)}
          </pre>
        </td>
      </tr>
    </>
  );
}
