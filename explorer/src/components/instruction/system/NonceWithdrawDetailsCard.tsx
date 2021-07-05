import React from "react";
import {
  SystemProgram,
  SignatureResult,
  ParsedInstruction,
} from "@solana/web3.js";
import { lamportsToSafeString } from "utils";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { WithdrawNonceInfo } from "./types";

export function NonceWithdrawDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: WithdrawNonceInfo;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Withdraw Nonce"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={SystemProgram.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Nonce Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.nonceAccount} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Authority Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.nonceAuthority} alignRight link />
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
