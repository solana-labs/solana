import { LAMPORTS_PER_SOL } from "@solana/web3.js";

export function assertUnreachable(x: never): never {
  throw new Error("Unreachable!");
}

export function lamportsToSolString(lamports: number): string {
  return `◎${(1.0 * Math.abs(lamports)) / LAMPORTS_PER_SOL}`;
}
