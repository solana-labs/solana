import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { InstructionCard } from "../InstructionCard";
import { AggregatePriceParams } from "./program";

export default function AggregatePriceDetailsCard({
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
  info: AggregatePriceParams;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Pyth: Update Price"
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
        <td>Price Account</td>
        <td className="text-lg-end">
          <Address pubkey={info.pricePubkey} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
