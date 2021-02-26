import React from "react";
import { ParsedInstruction, SignatureResult } from "@solana/web3.js";
import { coerce } from "superstruct";
import { ParsedInfo } from "validators";
import { VoteInfo } from "./types";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { displayTimestampUtc } from "utils/date";

export function VoteDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, innerCards, childIndex } = props;
  const parsed = coerce(props.ix.parsed, ParsedInfo);
  const info = coerce(parsed.info, VoteInfo);

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Vote"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={ix.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Vote Account</td>
        <td className="text-lg-right">
          <Address pubkey={info.voteAccount} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Vote Authority</td>
        <td className="text-lg-right">
          <Address pubkey={info.voteAuthority} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Clock Sysvar</td>
        <td className="text-lg-right">
          <Address pubkey={info.clockSysvar} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Slot Hashes Sysvar</td>
        <td className="text-lg-right">
          <Address pubkey={info.slotHashesSysvar} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Vote Hash</td>
        <td className="text-lg-right">
          <pre className="d-inline-block text-left mb-0">{info.vote.hash}</pre>
        </td>
      </tr>

      <tr>
        <td>Timestamp</td>
        <td className="text-lg-right text-monospace">
          {displayTimestampUtc(info.vote.timestamp * 1000)}
        </td>
      </tr>

      <tr>
        <td>Slots</td>
        <td className="text-lg-right text-monospace">
          <pre className="d-inline-block text-left mb-0">
            {info.vote.slots.join("\n")}
          </pre>
        </td>
      </tr>
    </InstructionCard>
  );
}
