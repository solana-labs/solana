import { TransactionInstruction } from "@solana/web3.js";

export const PROGRAM_IDS: string[] = [
  "LendZqTs7gn5CTSJU1jWKhKuVpjJGom45nnwPb2AMTi", // mainnet / testnet / devnet
];

const INSTRUCTION_LOOKUP: { [key: number]: string } = {
  0: "Initialize LendingMarket",
  1: "Initialize Reserve",
  2: "Initialize Obligation",
  3: "Reserve deposit",
  4: "Reserve withdraw",
  5: "Borrow",
  6: "Repay loan",
  7: "Liquidate loan",
  8: "Accrue interest",
};

export function isTokenLendingInstruction(
  instruction: TransactionInstruction
): boolean {
  return PROGRAM_IDS.includes(instruction.programId.toBase58());
}

export function parseTokenLendingInstructionTitle(
  instruction: TransactionInstruction
): string {
  const code = instruction.data[0];

  if (!(code in INSTRUCTION_LOOKUP)) {
    throw new Error(`Unrecognized Token Swap instruction code: ${code}`);
  }

  return INSTRUCTION_LOOKUP[code];
}
