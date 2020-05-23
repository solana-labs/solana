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

export function AllocateDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const { ix, index, result } = props;

  let params;
  try {
    params = SystemInstruction.decodeAllocate(ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

  const accountKey = params.accountPubkey.toBase58();

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Allocate Account"
    >
      <tr>
        <td>Program</td>
        <td className="text-right">
          <Copyable bottom right text={SystemProgram.programId.toBase58()}>
            <code>{displayAddress(SystemProgram.programId.toBase58())}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Account Address</td>
        <td className="text-right">
          <Copyable right text={accountKey}>
            <code>{accountKey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Allocated Space (Bytes)</td>
        <td className="text-right">{params.space}</td>
      </tr>
    </InstructionCard>
  );
}
