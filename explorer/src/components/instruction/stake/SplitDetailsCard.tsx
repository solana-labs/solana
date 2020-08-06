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

export function SplitDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const { ix, index, result } = props;

  let params;
  try {
    params = StakeInstruction.decodeSplit(ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

  return (
    <InstructionCard ix={ix} index={index} result={result} title="Split Stake">
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
        <td>New Stake Address</td>
        <td className="text-lg-right">
          <Address pubkey={params.splitStakePubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Split Amount (SOL)</td>
        <td className="text-lg-right">
          {lamportsToSolString(params.lamports)}
        </td>
      </tr>
    </InstructionCard>
  );
}
