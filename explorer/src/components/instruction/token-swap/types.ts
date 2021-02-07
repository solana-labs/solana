import { TransactionInstruction } from "@safecoin/web3.js";

export const PROGRAM_IDS: string[] = [
  "SwaPpA9LAaLfeLi3a68M4DjnLqgtticKg6CnyNwgAC8", // mainnet / testnet / devnet
  "9qvG1zUp8xF1Bi4m6UdRNby1BAAuaDrUxSpv4CmRRMjL", // mainnet - legacy
  "2n2dsFSgmPcZ8jkmBZLGUM2nzuFqcBGQ3JEEj6RJJcEg", // testnet - legacy
  "9tdctNJuFsYZ6VrKfKEuwwbPp4SFdFw3jYBZU8QUtzeX", // testnet - legacy
  "CrRvVBS4Hmj47TPU3cMukurpmCUYUrdHYxTQBxncBGqw", // testnet - legacy
  "BSfTAcBdqmvX5iE2PW88WFNNp2DHhLUaBKk5WrnxVkcJ", // devnet - legacy
  "H1E1G7eD5Rrcy43xvDxXCsjkRggz7MWNMLGJ8YNzJ8PM", // devnet - legacy
  "CMoteLxSPVPoc7Drcggf3QPg3ue8WPpxYyZTg77UGqHo", // devnet - legacy
  "EEuPz4iZA5reBUeZj6x1VzoiHfYeHMppSCnHZasRFhYo", // devnet - legacy
  "5rdpyt5iGfr68qt28hkefcFyF4WtyhTwqKDmHSBG8GZx", // localnet
];

const INSTRUCTION_LOOKUP: { [key: number]: string } = {
  0: "Initialize Swap",
  1: "Swap",
  2: "Deposit All Token Types",
  3: "Withdraw All Token Types",
  4: "Deposit Single Token Type Exact Amount In",
  5: "Withdraw Single Token Type Exact Amount Out",
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
