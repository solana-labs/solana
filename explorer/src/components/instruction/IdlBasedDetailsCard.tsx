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
import { camelToTitleCase } from "utils";
import { parseIxData } from "providers/idl/parsing";

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
    const ixDataParsed = parseIxData(ix.data, idl);
    if (ixDataParsed) ixName = camelToTitleCase(ixDataParsed.name);
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
