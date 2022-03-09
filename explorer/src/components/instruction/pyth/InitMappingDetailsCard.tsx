import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { InstructionCard } from "../InstructionCard";
import { InitMappingParams } from "./program";

export default function InitMappingDetailsCard({
  ix,
  index,
  result,
  info,
  innerCards,
  childIndex,
}: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: InitMappingParams;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Pyth: Init Mapping Account"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-end">
          <Address pubkey={ix.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Funding Account</td>
        <td className="text-lg-end">
          <Address pubkey={info.fundingPubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Mapping Account</td>
        <td className="text-lg-end">
          <Address pubkey={info.mappingPubkey} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
