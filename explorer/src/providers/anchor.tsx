import { useState } from "react";
import {
  Idl,
  Program,
  Provider,
  BorshAccountsCoder,
} from "@project-serum/anchor";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import { NodeWallet } from "@metaplex/js";
import { capitalizeFirstLetter } from "../utils/anchor";

/// Promises to fetch and decode anchor programs
let cachedAnchorProgramPromises: {
  [key: string]:
    | { _type: "promise"; promise: Promise<Program<Idl> | null> }
    | { _type: "result"; result: Program<Idl> | null };
} = {};

/// Promises to fetch and decode anchor accounts
let cachedAnchorAccountPromises: {
  [key: string]:
    | { _type: "promise"; promise: Promise<AnchorAccount | null> }
    | { _type: "result"; result: AnchorAccount | null };
} = {};

export function useAnchorProgram(
  programAddress: string,
  url: string
): Program | null {
  const key = `${programAddress}-${url}`;
  const cacheEntry = cachedAnchorProgramPromises[key];
  const [anchorProgram, setAnchorProgram] = useState<Program<Idl> | null>(() =>
    cacheEntry?._type === "result" ? cacheEntry.result : null
  );

  if (cacheEntry === undefined) {
    const promise = Program.at(
      programAddress,
      new Provider(new Connection(url), new NodeWallet(Keypair.generate()), {})
    )
      .then((program) => {
        cachedAnchorProgramPromises[key] = { _type: "result", result: program };
        setAnchorProgram(program);
        return program;
      })
      .catch((_) => {
        cachedAnchorProgramPromises[key] = { _type: "result", result: null };
        setAnchorProgram(null);
        return null;
      });
    cachedAnchorProgramPromises[key] = {
      _type: "promise",
      promise,
    };
    throw promise;
  } else if (cacheEntry._type === "promise") {
    cacheEntry.promise.then((result) => {
      setAnchorProgram(result);
    });
    throw cacheEntry.promise;
  }

  if (cacheEntry?._type === "result") {
    return cacheEntry.result;
  }
  return anchorProgram;
}

export type AnchorAccount = {
  layout: string;
  account: Object;
};
