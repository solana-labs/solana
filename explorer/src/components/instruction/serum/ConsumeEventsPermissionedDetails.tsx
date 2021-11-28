import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { ConsumeEventsPermissioned } from "./types";

export function ConsumeEventsPermissionedDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: ConsumeEventsPermissioned;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Serum Program: Consume Events Permissioned"
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
