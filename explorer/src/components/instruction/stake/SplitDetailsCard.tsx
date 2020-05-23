import React from "react";
import {
  TransactionInstruction,
  SignatureResult,
  StakeInstruction,
  StakeProgram
} from "@solana/web3.js";
import { displayAddress } from "utils/tx";
import { lamportsToSolString } from "utils";
import { InstructionCard } from "../InstructionCard";
import Copyable from "components/Copyable";
import { UnknownDetailsCard } from "../UnknownDetailsCard";

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

  const stakePubkey = params.stakePubkey.toBase58();
  const authorizedPubkey = params.authorizedPubkey.toBase58();
  const splitStakePubkey = params.splitStakePubkey.toBase58();

  return (
    <InstructionCard ix={ix} index={index} result={result} title="Split Stake">
      <tr>
        <td>Program</td>
        <td className="text-right">
          <Copyable bottom right text={StakeProgram.programId.toBase58()}>
            <code>{displayAddress(StakeProgram.programId.toBase58())}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Stake Address</td>
        <td className="text-right">
          <Copyable right text={stakePubkey}>
            <code>{stakePubkey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Authority Address</td>
        <td className="text-right">
          <Copyable right text={authorizedPubkey}>
            <code>{authorizedPubkey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>New Stake Address</td>
        <td className="text-right">
          <Copyable right text={splitStakePubkey}>
            <code>{splitStakePubkey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Split Amount (SOL)</td>
        <td className="text-right">{lamportsToSolString(params.lamports)}</td>
      </tr>
    </InstructionCard>
  );
}
