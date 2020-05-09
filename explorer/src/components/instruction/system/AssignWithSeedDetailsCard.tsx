import React from "react";
import {
  TransactionInstruction,
  SystemProgram,
  SignatureResult,
  SystemInstruction
} from "@solana/web3.js";
import { displayAddress } from "utils/tx";
import { InstructionCard } from "../InstructionCard";
import Copyable from "components/Copyable";
import { UnknownDetailsCard } from "../UnknownDetailsCard";

export function AssignWithSeedDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const { ix, index, result } = props;

  let params;
  try {
    params = SystemInstruction.decodeAssignWithSeed(ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

  const accountKey = params.accountPubkey.toBase58();
  const baseKey = params.basePubkey.toBase58();

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Assign Account w/ Seed"
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
        <td>Account Address</td>
        <td className="text-right">
          <Copyable text={accountKey}>
            <code>{accountKey}</code>
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
