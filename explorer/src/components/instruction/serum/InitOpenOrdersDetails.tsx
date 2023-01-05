import React from "react";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { InitOpenOrders, SerumIxDetailsProps } from "./types";

export function InitOpenOrdersDetailsCard(
  props: SerumIxDetailsProps<InitOpenOrders>
) {
  const { ix, index, result, programName, info, innerCards, childIndex } =
    props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={`${programName} Program: Init Open Orders`}
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
        <td>Market</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.market} alignRight link />
        </td>
      </tr>

      {info.accounts.openOrdersMarketAuthority && (
        <tr>
          <td>Open Orders Market Authority</td>
          <td className="text-lg-end">
            <Address
              pubkey={info.accounts.openOrdersMarketAuthority}
              alignRight
              link
            />
          </td>
        </tr>
      )}
    </InstructionCard>
  );
}
