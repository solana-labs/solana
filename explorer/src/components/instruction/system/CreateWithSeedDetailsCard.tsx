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

export function CreateWithSeedDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const { ix, index, result } = props;

  let params;
  try {
    params = SystemInstruction.decodeCreateWithSeed(ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

  const from = params.fromPubkey.toBase58();
  const newKey = params.newAccountPubkey.toBase58();
  const baseKey = params.basePubkey.toBase58();
  const [fromMeta, newMeta] = ix.keys;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Create Account w/ Seed"
    >
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
        <td>Base Address</td>
        <td className="text-right">
          <Copyable text={baseKey}>
            <code>{baseKey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Seed</td>
        <td className="text-right">
          <Copyable text={params.seed}>
            <code>{params.seed}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Transfer Amount (SOL)</td>
        <td className="text-right">{lamportsToSolString(params.lamports)}</td>
      </tr>

      <tr>
        <td>Allocated Space (Bytes)</td>
        <td className="text-right">{params.space}</td>
      </tr>

      <tr>
        <td>Assigned Owner</td>
        <td className="text-right">
          <Copyable text={params.programId.toBase58()}>
            <code>{displayAddress(params.programId)}</code>
          </Copyable>
        </td>
      </tr>
    </InstructionCard>
  );
}
