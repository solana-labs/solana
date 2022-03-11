import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { InstructionCard } from "../InstructionCard";
import { BasePublisherOperationParams } from "./program";

export default function BasePublisherOperationCard({
  ix,
  index,
  result,
  operationName,
  info,
  innerCards,
  childIndex,
}: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  operationName: string;
  info: BasePublisherOperationParams;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={`Pyth: ${operationName}`}
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
        <td>Price Account</td>
        <td className="text-lg-end">
          <Address pubkey={info.pricePubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Publisher</td>
        <td className="text-lg-end">
          <Address pubkey={info.publisherPubkey} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
