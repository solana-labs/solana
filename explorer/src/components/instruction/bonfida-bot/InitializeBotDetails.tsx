import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { InitializeBot } from "./types";

export function InitializeBotDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: InitializeBot;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Bonfida Bot: Initialize Bot"
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
        <td>Pool Account</td>
        <td className="text-lg-end">
          <Address pubkey={info.poolAccount} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Mint Account</td>
        <td className="text-lg-end">
          <Address pubkey={info.mintAccount} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Pool Seed</td>
        <td className="text-lg-end">{info.poolSeed}</td>
      </tr>

      <tr>
        <td>Max Number of Assets</td>
        <td className="text-lg-end">{info.maxNumberOfAsset}</td>
      </tr>

      <tr>
        <td>Number of Markets</td>
        <td className="text-lg-end">{info.numberOfMarkets}</td>
      </tr>
    </InstructionCard>
  );
}
