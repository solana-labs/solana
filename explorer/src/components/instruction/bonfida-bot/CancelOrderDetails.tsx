import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { CancelOrder } from "./types";

export function CancelOrderDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: CancelOrder;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Serum Program: Cancel Order"
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
        <td>Market</td>
        <td className="text-lg-end">
          <Address pubkey={info.market} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Signal Provider Address</td>
        <td className="text-lg-end">
          <Address pubkey={info.signalProviderKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Open Orders</td>
        <td className="text-lg-end">
          <Address pubkey={info.openOrdersKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Serum Event Queue</td>
        <td className="text-lg-end">
          <Address pubkey={info.serumEventQueue} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Serum Bids</td>
        <td className="text-lg-end">
          <Address pubkey={info.serumMarketBids} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Serum Asks</td>
        <td className="text-lg-end">
          <Address pubkey={info.serumMarketAsks} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Bot Address</td>
        <td className="text-lg-end">
          <Address pubkey={info.poolKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Serum Program ID</td>
        <td className="text-lg-end">
          <Address pubkey={info.dexProgramKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Pool Seed</td>
        <td className="text-lg-end">{info.poolSeed}</td>
      </tr>

      <tr>
        <td>Side</td>
        <td className="text-lg-end">{info.side}</td>
      </tr>

      <tr>
        <td>Order Id</td>
        <td className="text-lg-end">{info.orderId.toString(10)}</td>
      </tr>
    </InstructionCard>
  );
}
