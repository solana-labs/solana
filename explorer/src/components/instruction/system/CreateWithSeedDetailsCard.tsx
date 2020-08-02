import React from "react";
import {
  TransactionInstruction,
  SystemProgram,
  SignatureResult,
  SystemInstruction,
} from "@solana/web3.js";
import { lamportsToSolString } from "utils";
import { InstructionCard } from "../InstructionCard";
import Copyable from "components/Copyable";
import { UnknownDetailsCard } from "../UnknownDetailsCard";
import Address from "components/common/Address";

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
          <Address pubkey={SystemProgram.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>From Address</td>
        <td className="text-right">
          <Address pubkey={params.fromPubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>New Address</td>
        <td className="text-right">
          <Address pubkey={params.newAccountPubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Base Address</td>
        <td className="text-right">
          <Address pubkey={params.basePubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Seed</td>
        <td className="text-right">
          <Copyable right text={params.seed}>
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
          <Address pubkey={params.programId} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
