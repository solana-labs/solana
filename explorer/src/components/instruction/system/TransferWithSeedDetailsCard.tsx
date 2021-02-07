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
import { TransferWithSeedInfo } from "./types";

export function TransferWithSeedDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: TransferWithSeedInfo;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Transfer w/ Seed"
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
        <td>Destination Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.destination} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Base Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.sourceBase} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Transfer Amount (SAFE)</td>
        <td className="text-lg-right">{lamportsToSafeString(info.lamports)}</td>
      </tr>

      <tr>
        <td>Seed</td>
        <td className="text-lg-right">
          <Copyable right text={info.sourceSeed}>
            <code>{info.sourceSeed}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Source Owner</td>
        <td className="text-lg-right">
          <Address pubkey={info.sourceOwner} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
