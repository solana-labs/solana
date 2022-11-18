import React from "react";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { CancelOrderByClientId, SerumIxDetailsProps } from "./types";

export function CancelOrderByClientIdDetailsCard(
  props: SerumIxDetailsProps<CancelOrderByClientId>
) {
  const { ix, index, result, programName, info, innerCards, childIndex } =
    props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={`${programName} Program: Cancel Order By Client Id`}
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Market</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.market} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Open Orders</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.openOrders} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Request Queue</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.requestQueue} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Open Orders Owner</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.openOrdersOwner} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Client Id</td>
        <td className="text-lg-end">{info.data.clientId.toString(10)}</td>
      </tr>
    </InstructionCard>
  );
}
