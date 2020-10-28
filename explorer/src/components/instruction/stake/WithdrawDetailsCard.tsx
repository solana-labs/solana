import React from "react";
import {
  SignatureResult,
  StakeProgram,
  ParsedInstruction,
} from "@solana/web3.js";
import { lamportsToSolString } from "utils";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";

export function WithdrawDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: any;
}) {
  const { ix, index, result, info } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Withdraw Stake"
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={StakeProgram.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Stake Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.stakeAccount} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Authority Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.withdrawAuthority} alignRight link />
        </td>
      </tr>

      <tr>
        <td>To Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.destination} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Withdraw Amount (SOL)</td>
        <td className="text-lg-right">{lamportsToSolString(info.lamports)}</td>
      </tr>
    </InstructionCard>
  );
}
