import React from "react";
import {
  TransactionInstruction,
  SignatureResult,
  ParsedInstruction,
} from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import { programLabel } from "utils/tx";
import { useCluster } from "providers/cluster";
import { Idl } from "@project-serum/anchor";
import { decodeInstructionDataFromIdl } from "providers/idl";
import { camelToTitleCase } from "utils";

export function IdlBasedDetailsCard({
  ix,
  index,
  result,
  idl,
  innerCards,
  childIndex,
}: {
  ix: TransactionInstruction | ParsedInstruction;
  index: number;
  result: SignatureResult;
  idl: Idl;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { cluster } = useCluster();
  const programName =
    programLabel(ix.programId.toBase58(), cluster, idl) || "Unknown Program";
  let ixName = "Unknown Instruction";
  if (!("parsed" in ix) && idl) {
    const ixDataParsed = decodeInstructionDataFromIdl(ix.data, idl);
    if (ixDataParsed) ixName = camelToTitleCase(ixDataParsed.type);
  }
  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={`${programName}: ${ixName}`}
      innerCards={innerCards}
      childIndex={childIndex}
      defaultRaw
      idl={idl}
    />
  );
}
