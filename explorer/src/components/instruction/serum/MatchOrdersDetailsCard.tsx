import React from "react";
import { SignatureResult, TransactionInstruction } from "@safecoin/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { MatchOrders } from "./types";

export function MatchOrdersDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: MatchOrders;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Serum: Match Orders"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={info.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Market</td>
        <td className="text-lg-right">
          <Address pubkey={info.market} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Request Queue</td>
        <td className="text-lg-right">
          <Address pubkey={info.requestQueue} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Event Queue</td>
        <td className="text-lg-right">
          <Address pubkey={info.eventQueue} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Bids</td>
        <td className="text-lg-right">
          <Address pubkey={info.bids} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Asks</td>
        <td className="text-lg-right">
          <Address pubkey={info.asks} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Base Vault</td>
        <td className="text-lg-right">
          <Address pubkey={info.baseVault} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Quote Vault</td>
        <td className="text-lg-right">
          <Address pubkey={info.quoteVault} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Limit</td>
        <td className="text-lg-right">{info.limit}</td>
      </tr>
    </InstructionCard>
  );
}
