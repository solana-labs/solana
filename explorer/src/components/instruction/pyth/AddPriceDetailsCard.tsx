import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { InstructionCard } from "../InstructionCard";
import { AddPriceParams, PriceType } from "./program";

export default function AddPriceDetailsCard({
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
  info: AddPriceParams;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Pyth: Add Price Account"
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
        <td>Product Account</td>
        <td className="text-lg-end">
          <Address pubkey={info.productPubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Price Account</td>
        <td className="text-lg-end">
          <Address pubkey={info.pricePubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Exponent</td>
        <td className="text-lg-end">{info.exponent}</td>
      </tr>

      <tr>
        <td>Price Type</td>
        <td className="text-lg-end">{PriceType[info.priceType]}</td>
      </tr>
    </InstructionCard>
  );
}
