import {
  PublicKey,
  SystemProgram,
  StakeProgram,
  VOTE_PROGRAM_ID,
  BpfLoader,
  SYSVAR_CLOCK_PUBKEY,
  SYSVAR_RENT_PUBKEY,
  SYSVAR_REWARDS_PUBKEY,
  SYSVAR_STAKE_HISTORY_PUBKEY
} from "@solana/web3.js";

export function findGetParameter(parameterName: string): string | null {
  let result = null,
    tmp = [];
  window.location.search
    .substr(1)
    .split("&")
    .forEach(function(item) {
      tmp = item.split("=");
      if (tmp[0].toLowerCase() === parameterName.toLowerCase()) {
        if (tmp.length === 2) {
          result = decodeURIComponent(tmp[1]);
        } else if (tmp.length === 1) {
          result = "";
        }
      }
    });
  return result;
}

export function findPathSegment(pathName: string): string | null {
  const segments = window.location.pathname.substr(1).split("/");
  if (segments.length < 2) return null;

  // remove all but last two segments
  segments.splice(0, segments.length - 2);

  if (segments[0] === pathName) {
    return segments[1];
  }

  return null;
}

export function assertUnreachable(x: never): never {
  throw new Error("Unreachable!");
}

const PROGRAM_IDS = {
  Budget1111111111111111111111111111111111111: "Budget",
  Config1111111111111111111111111111111111111: "Config",
  Exchange11111111111111111111111111111111111: "Exchange",
  [StakeProgram.programId.toBase58()]: "Stake",
  Storage111111111111111111111111111111111111: "Storage",
  [SystemProgram.programId.toBase58()]: "System",
  Vest111111111111111111111111111111111111111: "Vest",
  [VOTE_PROGRAM_ID.toBase58()]: "Vote"
};

const LOADER_IDS = {
  MoveLdr111111111111111111111111111111111111: "Move Loader",
  NativeLoader1111111111111111111111111111111: "Native Loader",
  [BpfLoader.programId.toBase58()]: "BPF Loader"
};

const SYSVAR_IDS = {
  Sysvar1111111111111111111111111111111111111: "SYSVAR",
  [SYSVAR_CLOCK_PUBKEY.toBase58()]: "SYSVAR_CLOCK",
  SysvarEpochSchedu1e111111111111111111111111: "SYSVAR_EPOCH_SCHEDULE",
  SysvarFees111111111111111111111111111111111: "SYSVAR_FEES",
  SysvarRecentB1ockHashes11111111111111111111: "SYSVAR_RECENT_BLOCKHASHES",
  [SYSVAR_RENT_PUBKEY.toBase58()]: "SYSVAR_RENT",
  [SYSVAR_REWARDS_PUBKEY.toBase58()]: "SYSVAR_REWARDS",
  SysvarS1otHashes111111111111111111111111111: "SYSVAR_SLOT_HASHES",
  SysvarS1otHistory11111111111111111111111111: "SYSVAR_SLOT_HISTORY",
  [SYSVAR_STAKE_HISTORY_PUBKEY.toBase58()]: "SYSVAR_STAKE_HISTORY"
};

export function displayAddress(pubkey: PublicKey): string {
  const address = pubkey.toBase58();
  return (
    PROGRAM_IDS[address] ||
    LOADER_IDS[address] ||
    SYSVAR_IDS[address] ||
    address
  );
}
