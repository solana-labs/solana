import React from "react";
import { TransactionInstruction, SignatureResult } from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import { parseSerumInstructionTitle } from "utils/tx";

export function SerumDetailsCard({
  ix,
  index,
  result,
}: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const title = parseSerumInstructionTitle(ix);
  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={title}
      defaultRaw
    />
  );
}
