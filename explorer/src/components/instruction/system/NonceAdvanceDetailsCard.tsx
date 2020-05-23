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

export function NonceAdvanceDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const { ix, index, result } = props;

  let params;
  try {
    params = SystemInstruction.decodeNonceAdvance(ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

  const nonceKey = params.noncePubkey.toBase58();
  const authorizedKey = params.authorizedPubkey.toBase58();

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Advance Nonce"
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
        <td>Nonce Address</td>
        <td className="text-right">
          <Copyable right text={nonceKey}>
            <code>{nonceKey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Authority Address</td>
        <td className="text-right">
          <Copyable right text={authorizedKey}>
            <code>{authorizedKey}</code>
          </Copyable>
        </td>
      </tr>
    </InstructionCard>
  );
}
