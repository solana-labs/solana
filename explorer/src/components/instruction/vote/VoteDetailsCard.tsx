import React from "react";
import { PublicKey } from "@solana/web3.js";
import { create, Struct } from "superstruct";
import { ParsedInfo } from "validators";
import {
  UpdateCommissionInfo,
  UpdateValidatorInfo,
  VoteInfo,
  VoteSwitchInfo,
  WithdrawInfo,
  AuthorizeInfo,
} from "./types";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { displayTimestamp } from "utils/date";
import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { InstructionDetailsProps } from "components/transaction/InstructionsSection";
import { camelToTitleCase } from "utils";
import { useCluster } from "providers/cluster";
import { reportError } from "utils/sentry";

export function VoteDetailsCard(props: InstructionDetailsProps) {
  const { url } = useCluster();

  try {
    const parsed = create(props.ix.parsed, ParsedInfo);

    switch (parsed.type) {
      case "vote":
        return renderDetails<VoteInfo>(props, parsed, VoteInfo);
      case "authorize":
        return renderDetails<AuthorizeInfo>(props, parsed, AuthorizeInfo);
      case "withdraw":
        return renderDetails<WithdrawInfo>(props, parsed, WithdrawInfo);
      case "updateValidator":
        return renderDetails<UpdateValidatorInfo>(
          props,
          parsed,
          UpdateValidatorInfo
        );
      case "updateCommission":
        return renderDetails<UpdateCommissionInfo>(
          props,
          parsed,
          UpdateCommissionInfo
        );
      case "voteSwitch":
        return renderDetails<VoteSwitchInfo>(props, parsed, VoteSwitchInfo);
    }
  } catch (error) {
    reportError(error, {
      url,
    });
  }

  return <UnknownDetailsCard {...props} />;
}

function renderDetails<T>(
  props: InstructionDetailsProps,
  parsed: ParsedInfo,
  struct: Struct<T>
) {
  const info = create(parsed.info, struct);
  const attributes: JSX.Element[] = [];

  for (let [key, value] of Object.entries(info)) {
    if (value instanceof PublicKey) {
      value = <Address pubkey={value} alignRight link />;
    }

    if (key === "vote") {
      attributes.push(
        <tr key="vote-hash">
          <td>Vote Hash</td>
          <td className="text-lg-end">
            <pre className="d-inline-block text-start mb-0">{value.hash}</pre>
          </td>
        </tr>
      );

      if (value.timestamp) {
        attributes.push(
          <tr key="timestamp">
            <td>Timestamp</td>
            <td className="text-lg-end font-monospace">
              {displayTimestamp(value.timestamp * 1000)}
            </td>
          </tr>
        );
      }

      attributes.push(
        <tr key="vote-slots">
          <td>Slots</td>
          <td className="text-lg-end font-monospace">
            <pre className="d-inline-block text-start mb-0">
              {value.slots.join("\n")}
            </pre>
          </td>
        </tr>
      );
    } else {
      attributes.push(
        <tr key={key}>
          <td>{camelToTitleCase(key)} </td>
          <td className="text-lg-end">{value}</td>
        </tr>
      );
    }
  }

  return (
    <InstructionCard
      {...props}
      title={`Vote: ${camelToTitleCase(parsed.type)}`}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-end">
          <Address pubkey={props.ix.programId} alignRight link />
        </td>
      </tr>
      {attributes}
    </InstructionCard>
  );
}
