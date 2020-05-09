import React from "react";
import { TransactionInstruction, SignatureResult } from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";

export function UnknownDetailsCard({
  ix,
  index,
  result
}: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Unknown"
      defaultRaw
    />
  );
}
