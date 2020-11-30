import React from "react";
import { ParsedInstruction, TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { wrap } from "utils";

type RawParsedDetailsProps = {
  ix: ParsedInstruction;
  raw?: TransactionInstruction;
};

export function RawParsedDetails({ ix, raw }: RawParsedDetailsProps) {
  let hex = null;
  let b64 = null;
  if (raw) {
    hex = wrap(raw.data.toString("hex"), 50);
    b64 = wrap(raw.data.toString("base64"), 50);
  }

  return (
    <>
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={ix.programId} alignRight link />
        </td>
      </tr>

      {hex ? (
        <tr>
          <td>Instruction Data (Hex)</td>
          <td className="text-lg-right">
            <pre className="d-inline-block text-left mb-0">{hex}</pre>
          </td>
        </tr>
      ) : null}

      {b64 ? (
        <tr>
          <td>Instruction Data (Base64)</td>
          <td className="text-lg-right">
            <pre className="d-inline-block text-left mb-0">{b64}</pre>
          </td>
        </tr>
      ) : null}

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
