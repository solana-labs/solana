import React from "react";
import {
  TransactionInstruction,
  SystemProgram,
  SignatureResult,
  SystemInstruction
} from "@solana/web3.js";
import { lamportsToSolString } from "utils";
import { displayAddress } from "utils/tx";
import { InstructionCard } from "../InstructionCard";
import Copyable from "components/Copyable";
import { UnknownDetailsCard } from "../UnknownDetailsCard";

export function TransferDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const { ix, index, result } = props;

  let transfer;
  try {
    transfer = SystemInstruction.decodeTransfer(ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

  const from = transfer.fromPubkey.toBase58();
  const to = transfer.toPubkey.toBase58();
  const [fromMeta, toMeta] = ix.keys;
  return (
    <InstructionCard ix={ix} index={index} result={result} title="Transfer">
      <tr>
        <td>Program</td>
        <td className="text-right">
          <Copyable bottom text={SystemProgram.programId.toBase58()}>
            <code>{displayAddress(SystemProgram.programId)}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>
          <div className="mr-2 d-md-inline">From Address</div>
          {!fromMeta.isWritable && (
            <span className="badge badge-soft-dark mr-1">Readonly</span>
          )}
          {fromMeta.isSigner && (
            <span className="badge badge-soft-dark mr-1">Signer</span>
          )}
        </td>
        <td className="text-right">
          <Copyable text={from}>
            <code>{from}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>
          <div className="mr-2 d-md-inline">To Address</div>
          {!toMeta.isWritable && (
            <span className="badge badge-soft-dark mr-1">Readonly</span>
          )}
          {toMeta.isSigner && (
            <span className="badge badge-soft-dark mr-1">Signer</span>
          )}
        </td>
        <td className="text-right">
          <Copyable text={to}>
            <code>{to}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Transfer Amount (SOL)</td>
        <td className="text-right">{lamportsToSolString(transfer.lamports)}</td>
      </tr>
    </InstructionCard>
  );
}
