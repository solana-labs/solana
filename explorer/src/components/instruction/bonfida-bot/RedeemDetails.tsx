import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { Redeem } from "./types";

export function RedeemDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: Redeem;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Bonfida Bot: Redeem"
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
        <td>Source Bot Token Owner</td>
        <td className="text-lg-end">
          <Address pubkey={info.sourcePoolTokenKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Source Bot Token Address</td>
        <td className="text-lg-end">
          <Address pubkey={info.sourcePoolTokenKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Pool Seed</td>
        <td className="text-lg-end">{info.poolSeed}</td>
      </tr>

      <tr>
        <td>Pool Token Amount</td>
        <td className="text-lg-end">{info.poolTokenAmount}</td>
      </tr>
    </InstructionCard>
  );
}
