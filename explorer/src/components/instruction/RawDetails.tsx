import React from "react";
import bs58 from "bs58";
import { TransactionInstruction } from "@solana/web3.js";
import Copyable from "components/Copyable";
import Address from "components/common/Address";

function displayData(data: string) {
  if (data.length > 50) {
    return `${data.substring(0, 49)}…`;
  }
  return data;
}

export function RawDetails({ ix }: { ix: TransactionInstruction }) {
  const data = bs58.encode(ix.data);
  return (
    <>
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={ix.programId} alignRight link />
        </td>
      </tr>

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
        <td>Instruction Data (Base58)</td>
        <td className="text-lg-right">
          <Copyable text={data} right>
            <code>{displayData(data)}</code>
          </Copyable>
        </td>
      </tr>
    </>
  );
}
