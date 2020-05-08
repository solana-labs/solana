import React from "react";
import bs58 from "bs58";
import { TransactionInstruction, SignatureResult } from "@solana/web3.js";
import { displayAddress } from "utils/tx";
import { InstructionCard } from "./InstructionCard";
import Copyable from "components/Copyable";

export function RawDetailsCard({
  ix,
  index,
  result
}: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  return (
    <InstructionCard index={index} result={result} title="Raw">
      <tr>
        <td>Program</td>
        <td className="text-right">
          <Copyable bottom text={ix.programId.toBase58()}>
            <code>{displayAddress(ix.programId)}</code>
          </Copyable>
        </td>
      </tr>

      {ix.keys.map(({ pubkey, isSigner, isWritable }, keyIndex) => (
        <tr key={keyIndex}>
          <td>
            <div className="mr-2 d-md-inline">Account #{keyIndex + 1}</div>
            {!isWritable && (
              <span className="badge badge-soft-dark mr-1">Readonly</span>
            )}
            {isSigner && (
              <span className="badge badge-soft-dark mr-1">Signer</span>
            )}
          </td>
          <td className="text-right">
            <Copyable text={pubkey.toBase58()}>
              <code>{pubkey.toBase58()}</code>
            </Copyable>
          </td>
        </tr>
      ))}

      <tr>
        <td>Raw Data (Base58)</td>
        <td className="text-right">
          <Copyable text={bs58.encode(ix.data)}>
            <code>{bs58.encode(ix.data)}</code>
          </Copyable>
        </td>
      </tr>
    </InstructionCard>
  );
}
