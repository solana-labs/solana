import React from "react";
import {
  SignatureResult,
  StakeProgram,
  ParsedInstruction,
} from "@solana/web3.js";
import { lamportsToSafeString } from "utils";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { WithdrawInfo } from "./types";

export function WithdrawDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: WithdrawInfo;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Withdraw Stake"
      innerCards={innerCards}
      childIndex={childIndex}
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
        <td>Withdraw Amount (SAFE)</td>
        <td className="text-lg-right">{lamportsToSafeString(info.lamports)}</td>
      </tr>
    </InstructionCard>
  );
}
