import React from "react";
import {
  TransactionInstruction,
  SignatureResult,
  ParsedInstruction,
} from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";

export function UnknownDetailsCard({
  ix,
  index,
  result,
}: {
  ix: TransactionInstruction | ParsedInstruction;
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
