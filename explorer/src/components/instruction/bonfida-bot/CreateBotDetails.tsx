import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { CreateBot } from "./types";

export function CreateBotDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: CreateBot;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Bonfida Bot: Create Bot"
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
        <td>Bot Token Mint</td>
        <td className="text-lg-end">
          <Address pubkey={info.mintKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Bot Address</td>
        <td className="text-lg-end">
          <Address pubkey={info.poolKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Target Pool Token Address</td>
        <td className="text-lg-end">
          <Address pubkey={info.targetPoolTokenKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Serum Program ID</td>
        <td className="text-lg-end">
          <Address pubkey={info.serumProgramId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Signal Provider Address</td>
        <td className="text-lg-end">
          <Address pubkey={info.signalProviderKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Pool Seed</td>
        <td className="text-lg-end">{info.poolSeed}</td>
      </tr>

      <tr>
        <td>Fee Ratio</td>
        <td className="text-lg-end">{info.feeRatio}</td>
      </tr>

      <tr>
        <td>Fee Collection Period</td>
        <td className="text-lg-end">{info.feeCollectionPeriod}</td>
      </tr>

      <tr>
        <td>Serum Markets</td>
        <td className="text-lg-end">{info.markets}</td>
      </tr>

      <tr>
        <td>Deposit Amounts</td>
        <td className="text-lg-end">{info.depositAmounts}</td>
      </tr>
    </InstructionCard>
  );
}
