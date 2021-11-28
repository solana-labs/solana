import React from "react";
import { ParsedInstruction, SignatureResult } from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import { wrap } from "utils";
import { Address } from "components/common/Address";

export function MemoDetailsCard({
  ix,
  index,
  result,
  innerCards,
  childIndex,
}: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const data = wrap(ix.parsed, 50);
  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Memo Program: Memo"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-end">
          <Address pubkey={ix.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Data (UTF-8)</td>
        <td className="text-lg-end">
          <pre className="d-inline-block text-start mb-0">{data}</pre>
        </td>
      </tr>
    </InstructionCard>
  );
}
