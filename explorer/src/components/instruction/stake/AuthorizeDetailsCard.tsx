import React from "react";
import {
  SignatureResult,
  StakeProgram,
  ParsedInstruction,
} from "@safecoin/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { AuthorizeInfo } from "./types";

export function AuthorizeDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: AuthorizeInfo;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Stake Authorize"
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
        <td>Old Authority Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.authority} alignRight link />
        </td>
      </tr>

      <tr>
        <td>New Authority Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.newAuthority} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Authority Type</td>
        <td className="text-lg-right">{info.authorityType}</td>
      </tr>
    </InstructionCard>
  );
}
