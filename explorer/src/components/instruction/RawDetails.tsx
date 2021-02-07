import React from "react";
import { TransactionInstruction } from "@safecoin/web3.js";
import { Address } from "components/common/Address";
import { wrap } from "utils";

export function RawDetails({ ix }: { ix: TransactionInstruction }) {
  const data = wrap(ix.data.toString("hex"), 50);
  return (
    <>
      {ix.keys.map(({ pubkey, isSigner, isWritable }, keyIndex) => (
        <tr key={keyIndex}>
          <td>
            <div className="mr-2 d-md-inline">Account #{keyIndex + 1}</div>
            {!isWritable && (
              <span className="badge badge-soft-info mr-1">Readonly</span>
            )}
            {isSigner && (
              <span className="badge badge-soft-info mr-1">Signer</span>
            )}
          </td>
          <td className="text-lg-right">
            <Address pubkey={pubkey} alignRight link />
          </td>
        </tr>
      ))}

      <tr>
        <td>Instruction Data (Hex)</td>
        <td className="text-lg-right">
          <pre className="d-inline-block text-left mb-0">{data}</pre>
        </td>
      </tr>
    </>
  );
}
