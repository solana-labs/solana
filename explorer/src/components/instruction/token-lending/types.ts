import { TransactionInstruction } from "@solana/web3.js";

export const PROGRAM_IDS: string[] = [
  "LendZqTs7gn5CTSJU1jWKhKuVpjJGom45nnwPb2AMTi", // mainnet / testnet / devnet
];

const INSTRUCTION_LOOKUP: { [key: number]: string } = {
  0: "Initialize Lending Market",
  1: "Initialize Reserve",
  2: "Initialize Obligation",
  3: "Reserve Deposit",
  4: "Reserve Withdraw",
  5: "Borrow",
  6: "Repay Loan",
  7: "Liquidate Loan",
  8: "Accrue Interest",
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
    throw new Error(`Unrecognized Token Lending instruction code: ${code}`);
  }

  return INSTRUCTION_LOOKUP[code];
}
