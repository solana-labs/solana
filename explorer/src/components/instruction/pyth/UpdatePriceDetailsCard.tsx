import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { InstructionCard } from "../InstructionCard";
import { TradingStatus, UpdatePriceParams } from "./program";

export default function UpdatePriceDetailsCard({
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
  info: UpdatePriceParams;
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
        <td>Publisher</td>
        <td className="text-lg-end">
          <Address pubkey={info.publisherPubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Price Account</td>
        <td className="text-lg-end">
          <Address pubkey={info.pricePubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Status</td>
        <td className="text-lg-end">{TradingStatus[info.status]}</td>
      </tr>

      <tr>
        <td>Price</td>
        <td className="text-lg-end">{info.price}</td>
      </tr>

      <tr>
        <td>Conf</td>
        <td className="text-lg-end">{info.conf}</td>
      </tr>

      <tr>
        <td>Publish Slot</td>
        <td className="text-lg-end">{info.publishSlot}</td>
      </tr>
    </InstructionCard>
  );
}
