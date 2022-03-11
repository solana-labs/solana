import { TransactionInstruction } from "@solana/web3.js";

export const PROGRAM_IDS: string[] = [
  "gSbePebfvPy7tRqimPoVecS2UsBvYv46ynrzWocc92s", // devnet
  "8tfDNiaEyrV6Q1U4DEXrEigs9DoDtkugzFbybENEbCDz", // testnet
  "FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH", // mainnet
];

export function isPythInstruction(
  instruction: TransactionInstruction
): boolean {
  return PROGRAM_IDS.includes(instruction.programId.toBase58());
}
