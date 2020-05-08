import React from "react";
import {
  TransactionInstruction,
  SystemProgram,
  SignatureResult,
  SystemInstruction
} from "@solana/web3.js";
import { lamportsToSolString } from "utils";
import { displayAddress } from "utils/tx";
import { InstructionCard } from "./InstructionCard";
import Copyable from "components/Copyable";
import { RawDetailsCard } from "./RawDetailsCard";

export function CreateDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const { ix, index, result } = props;

  let create;
  try {
    create = SystemInstruction.decodeCreateAccount(ix);
  } catch (err) {
    console.error(err);
    return <RawDetailsCard {...props} />;
  }

  const from = create.fromPubkey.toBase58();
  const newKey = create.newAccountPubkey.toBase58();
  const [fromMeta, newMeta] = ix.keys;

  return (
    <InstructionCard index={index} result={result} title="Create Account">
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
          <div className="mr-2 d-md-inline">New Address</div>
          {!newMeta.isWritable && (
            <span className="badge badge-soft-dark mr-1">Readonly</span>
          )}
          {newMeta.isSigner && (
            <span className="badge badge-soft-dark mr-1">Signer</span>
          )}
        </td>
        <td className="text-right">
          <Copyable text={newKey}>
            <code>{newKey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Transfer Amount (SOL)</td>
        <td className="text-right">{lamportsToSolString(create.lamports)}</td>
      </tr>

      <tr>
        <td>Allocated Space (Bytes)</td>
        <td className="text-right">{create.space}</td>
      </tr>

      <tr>
        <td>Assigned Owner</td>
        <td className="text-right">
          <Copyable text={create.programId.toBase58()}>
            <code>{displayAddress(create.programId)}</code>
          </Copyable>
        </td>
      </tr>
    </InstructionCard>
  );
}
