import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { CollectFees } from "./types";

export function CollectFeesDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: CollectFees;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Bonfida Bot: Collect Fees"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-end">
          <Address pubkey={info.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Signal Provider</td>
        <td className="text-lg-end">
          <Address pubkey={info.signalProviderPoolTokenKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Insurance Fund</td>
        <td className="text-lg-end">
          <Address pubkey={info.bonfidaFeePoolTokenKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Buy and Burn</td>
        <td className="text-lg-end">
          <Address pubkey={info.bonfidaBnBPTKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Pool Seed</td>
        <td className="text-lg-end">{info.poolSeed}</td>
      </tr>
    </InstructionCard>
  );
}
