import React from "react";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { CancelOrderV2, SerumIxDetailsProps } from "./types";

export function CancelOrderV2DetailsCard(
  props: SerumIxDetailsProps<CancelOrderV2>
) {
  const { ix, index, result, programName, info, innerCards, childIndex } =
    props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={`${programName} Program: Cancel Order v2`}
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
          <Address pubkey={info.accounts.market} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Bids</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.bids} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Asks</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.asks} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Open Orders</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.openOrders} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Open Orders Owner</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.openOrdersOwner} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Event Queue</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.eventQueue} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Side</td>
        <td className="text-lg-end">{info.data.side}</td>
      </tr>

      <tr>
        <td>Order Id</td>
        <td className="text-lg-end">{info.data.orderId.toString(10)}</td>
      </tr>
    </InstructionCard>
  );
}
