import React from "react";
import {
  SignatureResult,
  StakeProgram,
  ParsedInstruction,
} from "@safecoin/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { DeactivateInfo } from "./types";

export function DeactivateDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: DeactivateInfo;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Deactivate Stake"
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
          <Address pubkey={info.stakeAuthority} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
