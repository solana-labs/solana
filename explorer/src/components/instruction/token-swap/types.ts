import { TransactionInstruction } from "@solana/web3.js";

export const PROGRAM_IDS: string[] = [
  "9qvG1zUp8xF1Bi4m6UdRNby1BAAuaDrUxSpv4CmRRMjL", // mainnet
  "2n2dsFSgmPcZ8jkmBZLGUM2nzuFqcBGQ3JEEj6RJJcEg", // testnet
  "9tdctNJuFsYZ6VrKfKEuwwbPp4SFdFw3jYBZU8QUtzeX", // testnet - legacy
  "CrRvVBS4Hmj47TPU3cMukurpmCUYUrdHYxTQBxncBGqw", // testnet - legacy
  "BSfTAcBdqmvX5iE2PW88WFNNp2DHhLUaBKk5WrnxVkcJ", // devnet
  "H1E1G7eD5Rrcy43xvDxXCsjkRggz7MWNMLGJ8YNzJ8PM", // devnet - legacy
  "CMoteLxSPVPoc7Drcggf3QPg3ue8WPpxYyZTg77UGqHo", // devnet - legacy
  "EEuPz4iZA5reBUeZj6x1VzoiHfYeHMppSCnHZasRFhYo", // devnet - legacy
  "5rdpyt5iGfr68qt28hkefcFyF4WtyhTwqKDmHSBG8GZx", // localnet
];

const INSTRUCTION_LOOKUP: { [key: number]: string } = {
  0: "Initialize Swap",
  1: "Exchange",
  2: "Deposit",
  3: "Withdraw",
};

export function isTokenSwapInstruction(
  instruction: TransactionInstruction
): boolean {
  return PROGRAM_IDS.includes(instruction.programId.toBase58());
}

export function parseTokenSwapInstructionTitle(
  instruction: TransactionInstruction
): string {
  const code = instruction.data[0];

  if (!(code in INSTRUCTION_LOOKUP)) {
    throw new Error(`Unrecognized Token Swap instruction code: ${code}`);
  }

  return INSTRUCTION_LOOKUP[code];
}
