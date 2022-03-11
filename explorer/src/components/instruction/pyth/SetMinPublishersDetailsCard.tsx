import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { InstructionCard } from "../InstructionCard";
import { SetMinPublishersParams } from "./program";

export default function SetMinPublishersDetailsCard({
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
  info: SetMinPublishersParams;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Pyth: Set Minimum Number Of Publishers"
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

      <tr>
        <td>Min Publishers</td>
        <td className="text-lg-end">{info.minPublishers}</td>
      </tr>
    </InstructionCard>
  );
}
