import React from "react";
import {
  SystemProgram,
  SignatureResult,
  ParsedInstruction,
} from "@safecoin/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { AllocateInfo } from "./types";

export function AllocateDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: AllocateInfo;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Allocate Account"
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
        <td>Account Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.account} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Allocated Space (Bytes)</td>
        <td className="text-lg-right">{info.space}</td>
      </tr>
    </InstructionCard>
  );
}
