import { LAMPORTS_PER_SOL } from "@solana/web3.js";

export function assertUnreachable(x: never): never {
  throw new Error("Unreachable!");
}

export function lamportsToSolString(
  lamports: number,
  maximumFractionDigits: number = 9
): string {
  const sol = Math.abs(lamports) / LAMPORTS_PER_SOL;
  return (
    "◎" + new Intl.NumberFormat("en-US", { maximumFractionDigits }).format(sol)
  );
}
