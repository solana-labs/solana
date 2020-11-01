import { MARKETS } from "@project-serum/serum";
import { TransactionInstruction } from "@solana/web3.js";

const SERUM_PROGRAM_ID = "4ckmDgGdxQoPDLUkDT3vHgSAkzA3QRdNq5ywwY4sUSJn";

export function isSerumInstruction(instruction: TransactionInstruction) {
  return (
    instruction.programId.toBase58() === SERUM_PROGRAM_ID ||
    MARKETS.some(
      (market) =>
        market.programId && market.programId.equals(instruction.programId)
    )
  );
}

const SERUM_CODE_LOOKUP: { [key: number]: string } = {
  0: "Initialize Market",
  1: "New Order",
  2: "Match Orders",
  3: "Consume Events",
  4: "Cancel Order",
  5: "Settle Funds",
  6: "Cancel Order By Client Id",
  7: "Disable Market",
  8: "Sweep Fees",
};

export function parseSerumInstructionTitle(
  instruction: TransactionInstruction
): string {
  const code = instruction.data.slice(1, 5).readUInt32LE(0);

  if (!(code in SERUM_CODE_LOOKUP)) {
    throw new Error(`Unrecognized Serum instruction code: ${code}`);
  }

  return SERUM_CODE_LOOKUP[code];
}
