import React from "react";
import {
  SystemProgram,
  SignatureResult,
  ParsedInstruction,
} from "@solana/web3.js";
import { SolBalance } from "utils";
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
      title="System Program: Transfer"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-end">
          <Address pubkey={SystemProgram.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>From Address</td>
        <td className="text-lg-end">
          <Address pubkey={info.source} alignRight link />
        </td>
      </tr>

      <tr>
        <td>To Address</td>
        <td className="text-lg-end">
          <Address pubkey={info.destination} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Transfer Amount (SOL)</td>
        <td className="text-lg-end">
          <SolBalance lamports={info.lamports} />
        </td>
      </tr>
    </InstructionCard>
  );
}
