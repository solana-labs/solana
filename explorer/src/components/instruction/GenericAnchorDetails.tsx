import {
  SignatureResult,
  TransactionInstruction,
} from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import {
  Idl,
  Program,
} from "@project-serum/anchor";
import { getAnchorNameForInstruction, getProgramName } from "utils/anchor";

export function AnchorDetailsCard(props: {
  key: string,
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  signature: string;
  innerCards?: JSX.Element[];
  childIndex?: number;
  program: Program<Idl>
}) {
  const ixName = getAnchorNameForInstruction(props.ix, props.program) ?? getProgramName(props.program) ?? "Unknown Program: Unknown Instruction";
  return (
      <InstructionCard title={ixName} defaultRaw {...props} />
  );
}