import bs58 from "bs58";
import {
  SystemProgram,
  StakeProgram,
  VOTE_PROGRAM_ID,
  BPF_LOADER_PROGRAM_ID,
  BPF_LOADER_DEPRECATED_PROGRAM_ID,
  SYSVAR_CLOCK_PUBKEY,
  SYSVAR_RENT_PUBKEY,
  SYSVAR_REWARDS_PUBKEY,
  SYSVAR_STAKE_HISTORY_PUBKEY,
  ParsedTransaction,
  TransactionInstruction,
  Transaction,
} from "@solana/web3.js";
import { TokenRegistry } from "tokenRegistry";
import { Cluster } from "providers/cluster";
import { SerumMarketRegistry } from "serumMarketRegistry";

export type ProgramName = typeof PROGRAM_IDS[keyof typeof PROGRAM_IDS];

export const PROGRAM_IDS = {
  BrEAK7zGZ6dM71zUDACDqJnekihmwF15noTddWTsknjC: "Break Solana Program",
  Budget1111111111111111111111111111111111111: "Budget Program",
  Config1111111111111111111111111111111111111: "Config Program",
  Exchange11111111111111111111111111111111111: "Exchange Program",
  [StakeProgram.programId.toBase58()]: "Stake Program",
  Storage111111111111111111111111111111111111: "Storage Program",
  [SystemProgram.programId.toBase58()]: "System Program",
  Vest111111111111111111111111111111111111111: "Vest Program",
  [VOTE_PROGRAM_ID.toBase58()]: "Vote Program",
  TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA: "SPL Token Program",
  ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL:
    "SPL Associated Token Account Program",
} as const;

export type LoaderName = typeof LOADER_IDS[keyof typeof LOADER_IDS];
export const LOADER_IDS = {
  MoveLdr111111111111111111111111111111111111: "Move Loader",
  NativeLoader1111111111111111111111111111111: "Native Loader",
  [BPF_LOADER_DEPRECATED_PROGRAM_ID.toBase58()]: "BPF Loader",
  [BPF_LOADER_PROGRAM_ID.toBase58()]: "BPF Loader 2",
} as const;

const SYSVAR_ID: { [key: string]: string } = {
  Sysvar1111111111111111111111111111111111111: "SYSVAR",
};

export const SYSVAR_IDS = {
  [SYSVAR_CLOCK_PUBKEY.toBase58()]: "Sysvar: Clock",
  SysvarEpochSchedu1e111111111111111111111111: "Sysvar: Epoch Schedule",
  SysvarFees111111111111111111111111111111111: "Sysvar: Fees",
  SysvarRecentB1ockHashes11111111111111111111: "Sysvar: Recent Blockhashes",
  [SYSVAR_RENT_PUBKEY.toBase58()]: "Sysvar: Rent",
  [SYSVAR_REWARDS_PUBKEY.toBase58()]: "Sysvar: Rewards",
  SysvarS1otHashes111111111111111111111111111: "Sysvar: Slot Hashes",
  SysvarS1otHistory11111111111111111111111111: "Sysvar: Slot History",
  [SYSVAR_STAKE_HISTORY_PUBKEY.toBase58()]: "Sysvar: Stake History",
};

export function addressLabel(
  address: string,
  cluster: Cluster
): string | undefined {
  return (
    PROGRAM_IDS[address] ||
    LOADER_IDS[address] ||
    SYSVAR_IDS[address] ||
    SYSVAR_ID[address] ||
    TokenRegistry.get(address, cluster)?.name ||
    SerumMarketRegistry.get(address, cluster)
  );
}

export function displayAddress(address: string, cluster: Cluster): string {
  return addressLabel(address, cluster) || address;
}

export function intoTransactionInstruction(
  tx: ParsedTransaction,
  index: number
): TransactionInstruction | undefined {
  const message = tx.message;
  const instruction = message.instructions[index];
  if ("parsed" in instruction) return;

  const keys = [];
  for (const account of instruction.accounts) {
    const accountKey = message.accountKeys.find(({ pubkey }) =>
      pubkey.equals(account)
    );
    if (!accountKey) return;
    keys.push({
      pubkey: accountKey.pubkey,
      isSigner: accountKey.signer,
      isWritable: accountKey.writable,
    });
  }

  return new TransactionInstruction({
    data: bs58.decode(instruction.data),
    keys: keys,
    programId: instruction.programId,
  });
}

export function intoParsedTransaction(tx: Transaction): ParsedTransaction {
  const message = tx.compileMessage();
  return {
    signatures: tx.signatures.map((value) =>
      bs58.encode(value.signature as any)
    ),
    message: {
      accountKeys: message.accountKeys.map((key, index) => ({
        pubkey: key,
        signer: tx.signatures.some(({ publicKey }) => publicKey.equals(key)),
        writable: message.isAccountWritable(index),
      })),
      instructions: message.instructions.map((ix) => ({
        programId: message.accountKeys[ix.programIdIndex],
        accounts: ix.accounts.map((index) => message.accountKeys[index]),
        data: ix.data,
      })),
      recentBlockhash: message.recentBlockhash,
    },
  };
}
