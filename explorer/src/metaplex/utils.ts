/*
    Taken from: https://github.com/metaplex-foundation/metaplex/blob/master/js/packages/common/src/utils/utils.ts
*/

import { PublicKey } from "@solana/web3.js";

export const findProgramAddress = async (
  seeds: (Buffer | Uint8Array)[],
  programId: PublicKey
) => {
  const key =
    "pda-" +
    seeds.reduce((agg, item) => agg + item.toString("hex"), "") +
    programId.toString();
  const cached = localStorage.getItem(key);
  if (cached) {
    const value = JSON.parse(cached);

    return [value.key, parseInt(value.nonce)] as [string, number];
  }

  const result = await PublicKey.findProgramAddress(seeds, programId);

  try {
    localStorage.setItem(
      key,
      JSON.stringify({
        key: result[0].toBase58(),
        nonce: result[1],
      })
    );
  } catch {
    // ignore
  }

  return [result[0].toBase58(), result[1]] as [string, number];
};

export const pubkeyToString = (key: PublicKey | string = "") => {
  return typeof key === "string" ? key : key?.toBase58() || "";
};

export const getLast = <T>(arr: T[]) => {
  if (arr.length <= 0) {
    return undefined;
  }

  return arr[arr.length - 1];
};
