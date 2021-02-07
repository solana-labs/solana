import React from "react";
import {
  SystemProgram,
  SignatureResult,
  ParsedInstruction,
} from "@safecoin/web3.js";
import { lamportsToSafeString } from "utils";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { TransferInfo } from "./types";

export function TransferDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: TransferInfo;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Transfer"
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
        <td>Transfer Amount (SAFE)</td>
        <td className="text-lg-right">{lamportsToSafeString(info.lamports)}</td>
      </tr>
    </InstructionCard>
  );
}
