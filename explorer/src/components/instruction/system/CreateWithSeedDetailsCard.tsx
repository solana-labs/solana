import React from "react";
import {
  SystemProgram,
  SignatureResult,
  ParsedInstruction,
} from "@safecoin/web3.js";
import { lamportsToSafeString } from "utils";
import { InstructionCard } from "../InstructionCard";
import { Copyable } from "components/common/Copyable";
import { Address } from "components/common/Address";
import { CreateAccountWithSeedInfo } from "./types";

export function CreateWithSeedDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: CreateAccountWithSeedInfo;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Create Account w/ Seed"
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
        <td>Base Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.base} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Seed</td>
        <td className="text-lg-right">
          <Copyable right text={info.seed}>
            <code>{info.seed}</code>
          </Copyable>
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
