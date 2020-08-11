import React from "react";
import { ParsedInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";

export function RawParsedDetails({ ix }: { ix: ParsedInstruction }) {
  return (
    <>
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={ix.programId} alignRight link />
        </td>
      </tr>

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
