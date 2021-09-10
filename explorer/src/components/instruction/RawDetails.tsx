import React from "react";
import { TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";

export function RawDetails({ ix }: { ix: TransactionInstruction }) {
  const data = ix.data.toString("hex");
  return (
    <>
      {ix.keys.map(({ pubkey, isSigner, isWritable }, keyIndex) => (
        <tr key={keyIndex}>
          <td>
            <div className="mr-2 d-md-inline">Account #{keyIndex + 1}</div>
            {isWritable && (
              <span className="badge badge-soft-info mr-1">Writable</span>
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
        <td>
          Instruction Data <span className="text-muted">(Hex)</span>
        </td>
        <td className="text-lg-right">
          <pre className="d-inline-block text-left mb-0 data-wrap">{data}</pre>
        </td>
      </tr>
    </>
  );
}
