import React from "react";
import {
  TransactionInstruction,
  SignatureResult,
  StakeInstruction,
  StakeProgram,
} from "@solana/web3.js";
import { lamportsToSolString } from "utils";
import { InstructionCard } from "../InstructionCard";
import { UnknownDetailsCard } from "../UnknownDetailsCard";
import Address from "components/common/Address";

export function WithdrawDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const { ix, index, result } = props;

  let params;
  try {
    params = StakeInstruction.decodeWithdraw(ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

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
          <Address pubkey={params.stakePubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Authority Address</td>
        <td className="text-lg-right">
          <Address pubkey={params.authorizedPubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>To Address</td>
        <td className="text-lg-right">
          <Address pubkey={params.toPubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Withdraw Amount (SOL)</td>
        <td className="text-lg-right">
          {lamportsToSolString(params.lamports)}
        </td>
      </tr>
    </InstructionCard>
  );
}
