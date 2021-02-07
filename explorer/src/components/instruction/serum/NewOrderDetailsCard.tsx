import React from "react";
import { SignatureResult, TransactionInstruction } from "@safecoin/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { NewOrder } from "./types";

export function NewOrderDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: NewOrder;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Serum: New Order"
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
        <td>Open Orders</td>
        <td className="text-lg-right">
          <Address pubkey={info.openOrders} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Request Queue</td>
        <td className="text-lg-right">
          <Address pubkey={info.requestQueue} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Payer</td>
        <td className="text-lg-right">
          <Address pubkey={info.payer} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Owner</td>
        <td className="text-lg-right">
          <Address pubkey={info.owner} alignRight link />
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
        <td>Side</td>
        <td className="text-lg-right">{info.side}</td>
      </tr>

      <tr>
        <td>Order Type</td>
        <td className="text-lg-right">{info.orderType}</td>
      </tr>

      <tr>
        <td>Limit Price</td>
        <td className="text-lg-right">{info.limitPrice.toString(10)}</td>
      </tr>

      <tr>
        <td>Max Quantity</td>
        <td className="text-lg-right">{info.maxQuantity.toString(10)}</td>
      </tr>

      <tr>
        <td>Client Id</td>
        <td className="text-lg-right">{info.clientId.toString(10)}</td>
      </tr>
    </InstructionCard>
  );
}
