import React from "react";
import {
  SystemProgram,
  SignatureResult,
  ParsedInstruction,
} from "@solana/web3.js";
import { lamportsToSolString } from "utils";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";

export function TransferDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: any;
}) {
  const { ix, index, result, info } = props;

  return (
    <InstructionCard ix={ix} index={index} result={result} title="Transfer">
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={SystemProgram.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>From Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.source} alignRight link />
        </td>
      </tr>

      <tr>
        <td>To Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.destination} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Transfer Amount (SOL)</td>
        <td className="text-lg-right">{lamportsToSolString(info.lamports)}</td>
      </tr>
    </InstructionCard>
  );
}
