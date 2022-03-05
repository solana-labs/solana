import { TransactionInstruction } from "@solana/web3.js";

// list of programs written in anchor
// - should have idl on-chain for GenericAnchorDetailsCard to work out of the box
// - before adding another program to this list, please make sure that the ix
// are decoding without any errors
const knownAnchorPrograms = [
  // https://github.com/blockworks-foundation/voter-stake-registry
  "4Q6WW2ouZ6V3iaNm56MTd5n2tnTm4C5fiH8miFHnAFHo",
];

export const isInstructionFromAnAnchorProgram = (
  instruction: TransactionInstruction
) => {
  return knownAnchorPrograms.includes(instruction.programId.toBase58());
};
