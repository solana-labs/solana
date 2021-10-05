/*
    Taken from: https://github.com/metaplex-foundation/metaplex/blob/master/js/packages/common/src/utils/utils.ts
*/

import { PublicKey } from "@solana/web3.js";

export const pubkeyToString = (key: PublicKey | string = "") => {
  return typeof key === "string" ? key : key?.toBase58() || "";
};

export const getLast = <T>(arr: T[]) => {
  if (arr.length <= 0) {
    return undefined;
  }

  return arr[arr.length - 1];
};
