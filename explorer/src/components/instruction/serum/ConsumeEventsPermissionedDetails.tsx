import React from "react";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { ConsumeEventsPermissioned, SerumIxDetailsProps } from "./types";

export function ConsumeEventsPermissionedDetailsCard(
  props: SerumIxDetailsProps<ConsumeEventsPermissioned>
) {
  const { ix, index, result, programName, info, innerCards, childIndex } =
    props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={`${programName} Program: Consume Events Permissioned`}
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
        <td>Event Queue</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.eventQueue} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Crank Authority</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.crankAuthority} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Open Orders Accounts</td>
        <td className="text-lg-end">
          {info.accounts.openOrders.map((account, index) => {
            return <Address pubkey={account} key={index} alignRight link />;
          })}
        </td>
      </tr>

      <tr>
        <td>Limit</td>
        <td className="text-lg-end">{info.data.limit}</td>
      </tr>
    </InstructionCard>
  );
}
