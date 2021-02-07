import React from "react";
import {
  TransactionInstruction,
  SignatureResult,
  ParsedInstruction,
} from "@safecoin/web3.js";
import { InstructionCard } from "./InstructionCard";

export function UnknownDetailsCard({
  ix,
  index,
  result,
  innerCards,
  childIndex,
}: {
  ix: TransactionInstruction | ParsedInstruction;
  index: number;
  result: SignatureResult;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Unknown"
      innerCards={innerCards}
      childIndex={childIndex}
      defaultRaw
    />
  );
}
