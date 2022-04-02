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

export function useAnchorAccount(
  accountPubkey: string,
  programId: string,
  url: string
): AnchorAccount | null {
  const key = `${accountPubkey}-${url}`;
  const cacheEntry = cachedAnchorAccountPromises[key];
  const [anchorAccount, setAnchorAccount] = useState<AnchorAccount | null>(() =>
    cacheEntry?._type === "result" ? cacheEntry.result : null
  );
  const program = useAnchorProgram(programId, url);
  if (!program) return null;

  if (cacheEntry === undefined) {
    let fetchPromise = async () => {
      let decodedAnchorObject: Object | null = null;
      const accountInfo = await new Connection(url).getAccountInfo(
        new PublicKey(accountPubkey)
      );
      if (accountInfo && accountInfo.data) {
        const discriminator = accountInfo?.data.slice(0, 8);
        if (!discriminator) {
          return null;
        }
        // Iterate all the structs, see if any of the name-hashes match
        Object.keys(program.account).forEach((accountType) => {
          const layoutName = capitalizeFirstLetter(accountType);
          const discriminatorToCheck =
            BorshAccountsCoder.accountDiscriminator(layoutName);

          if (equal(discriminatorToCheck, discriminator)) {
            const accountDecoder = program.account[accountType];
            decodedAnchorObject = {
              layout: layoutName,
              account: accountDecoder.coder.accounts.decode(
                layoutName,
                accountInfo.data
              ),
            };
          }
        });
      }
      return decodedAnchorObject;
    };
    const promise = fetchPromise().then((account) => {
      cachedAnchorAccountPromises[key] = { _type: "result", result: account };
      setAnchorAccount(account);
      return account;
    });
    cachedAnchorAccountPromises[key] = {
      _type: "promise",
      promise,
    };
  } else if (cacheEntry._type === "promise") {
    cacheEntry.promise.then((account) => {
      setAnchorAccount(account);
      return account;
    });
  }
  return anchorAccount;
}

function equal(buf1: Buffer, buf2: Buffer) {
  if (buf1.byteLength !== buf2.byteLength) return false;
  var dv1 = new Int8Array(buf1);
  var dv2 = new Int8Array(buf2);
  for (var i = 0; i !== buf1.byteLength; i++) {
    if (dv1[i] !== dv2[i]) return false;
  }
  return true;
}
