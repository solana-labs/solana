import React from "react";
import { ParsedInstruction, SignatureResult } from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import { wrap } from "utils";

export function MemoDetailsCard({
  ix,
  index,
  result,
}: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
}) {
  const data = wrap(ix.parsed, 50);
  return (
    <InstructionCard ix={ix} index={index} result={result} title="Memo">
      <tr>
        <td>Data (UTF-8)</td>
        <td className="text-lg-right">
          <pre className="d-inline-block text-left mb-0">{data}</pre>
        </td>
      </tr>
    </InstructionCard>
  );
}
