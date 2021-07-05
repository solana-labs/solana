import React from "react";
import {
  SystemProgram,
  SignatureResult,
  ParsedInstruction,
} from "@solana/web3.js";
import { lamportsToSafeString } from "utils";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { CreateAccountInfo } from "./types";

export function CreateDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: CreateAccountInfo;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Create Account"
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
        <td>New Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.newAccount} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Transfer Amount (SAFE)</td>
        <td className="text-lg-right">{lamportsToSafeString(info.lamports)}</td>
      </tr>

      <tr>
        <td>Allocated Space (Bytes)</td>
        <td className="text-lg-right">{info.space}</td>
      </tr>

      <tr>
        <td>Assigned Owner</td>
        <td className="text-lg-right">
          <Address pubkey={info.owner} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
