import React from "react";
import { ParsedInstruction } from "@safecoin/web3.js";

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
        <td>Instruction Data (JSON)</td>
        <td className="text-lg-right">
          <pre className="d-inline-block text-left">
            {JSON.stringify(ix.parsed, null, 2)}
          </pre>
        </td>
      </tr>
    </>
  );
}
