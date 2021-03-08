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
  PartiallyDecodedInstruction,
  ParsedInstruction,
} from "@solana/web3.js";
import { Cluster } from "providers/cluster";
import { SerumMarketRegistry } from "serumMarketRegistry";
import { TokenInfoMap } from "@solana/spl-token-registry";

export type ProgramName = typeof PROGRAM_NAME_BY_ID[keyof typeof PROGRAM_NAME_BY_ID];

export enum PROGRAM_NAMES {
  BREAK_SOLANA = "Break Solana Program",
  BUDGET = "Budget Program",
  CONFIG = "Config Program",
  EXCHANGE = "Exchange Program",
  STAKE = "Stake Program",
  STORAGE = "Storage Program",
  SYSTEM = "System Program",
  VEST = "Vest Program",
  VOTE = "Vote Program",
  SPL_TOKEN = "SPL Token Program",
  ASSOCIATED_TOKEN = "SPL Associated Token Program",
  MEMO = "Memo Program",
  MEMO_2 = "Memo Program 2",
  SWAP = "Swap Program",
  LENDING = "Lending Program",
  SERUM_2 = "Serum Program v2",
  SERUM_3 = "Serum Program v3",
}

export const SEARCHABLE_PROGRAMS: ProgramName[] = [
  PROGRAM_NAMES.BREAK_SOLANA,
  PROGRAM_NAMES.BUDGET,
  PROGRAM_NAMES.CONFIG,
  PROGRAM_NAMES.EXCHANGE,
  PROGRAM_NAMES.STAKE,
  PROGRAM_NAMES.STORAGE,
  PROGRAM_NAMES.SYSTEM,
  PROGRAM_NAMES.VEST,
  PROGRAM_NAMES.VOTE,
  PROGRAM_NAMES.SPL_TOKEN,
  PROGRAM_NAMES.ASSOCIATED_TOKEN,
  PROGRAM_NAMES.MEMO,
  PROGRAM_NAMES.MEMO_2,
  PROGRAM_NAMES.SWAP,
  PROGRAM_NAMES.LENDING,
  PROGRAM_NAMES.SERUM_2,
  PROGRAM_NAMES.SERUM_3,
];

export const PROGRAM_NAME_BY_ID = {
  BrEAK7zGZ6dM71zUDACDqJnekihmwF15noTddWTsknjC: PROGRAM_NAMES.BREAK_SOLANA,
  Budget1111111111111111111111111111111111111: PROGRAM_NAMES.BUDGET,
  Config1111111111111111111111111111111111111: PROGRAM_NAMES.CONFIG,
  Exchange11111111111111111111111111111111111: PROGRAM_NAMES.EXCHANGE,
  [StakeProgram.programId.toBase58()]: PROGRAM_NAMES.STAKE,
  Storage111111111111111111111111111111111111: PROGRAM_NAMES.STORAGE,
  [SystemProgram.programId.toBase58()]: PROGRAM_NAMES.SYSTEM,
  Vest111111111111111111111111111111111111111: PROGRAM_NAMES.VEST,
  [VOTE_PROGRAM_ID.toBase58()]: PROGRAM_NAMES.VOTE,
  TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA: PROGRAM_NAMES.SPL_TOKEN,
  ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL: PROGRAM_NAMES.ASSOCIATED_TOKEN,
  Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo: PROGRAM_NAMES.MEMO,
  MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr: PROGRAM_NAMES.MEMO_2,
  SwaPpA9LAaLfeLi3a68M4DjnLqgtticKg6CnyNwgAC8: PROGRAM_NAMES.SWAP,
  LendZqTs7gn5CTSJU1jWKhKuVpjJGom45nnwPb2AMTi: PROGRAM_NAMES.LENDING,
  EUqojwWA2rd19FZrzeBncJsm38Jm1hEhE3zsmX3bRc2o: PROGRAM_NAMES.SERUM_2,
  "9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin": PROGRAM_NAMES.SERUM_3,
} as const;

export type LoaderName = typeof LOADER_IDS[keyof typeof LOADER_IDS];
export const LOADER_IDS = {
  MoveLdr111111111111111111111111111111111111: "Move Loader",
  NativeLoader1111111111111111111111111111111: "Native Loader",
  [BPF_LOADER_DEPRECATED_PROGRAM_ID.toBase58()]: "BPF Loader",
  [BPF_LOADER_PROGRAM_ID.toBase58()]: "BPF Loader 2",
  BPFLoaderUpgradeab1e11111111111111111111111: "BPF Upgradeable Loader",
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
  cluster: Cluster,
  tokenRegistry?: TokenInfoMap
): string | undefined {
  return (
    PROGRAM_NAME_BY_ID[address] ||
    LOADER_IDS[address] ||
    SYSVAR_IDS[address] ||
    SYSVAR_ID[address] ||
    tokenRegistry?.get(address)?.name ||
    SerumMarketRegistry.get(address, cluster)
  );
}

export function displayAddress(
  address: string,
  cluster: Cluster,
  tokenRegistry: TokenInfoMap
): string {
  return addressLabel(address, cluster, tokenRegistry) || address;
}

export function intoTransactionInstruction(
  tx: ParsedTransaction,
  instruction: ParsedInstruction | PartiallyDecodedInstruction
): TransactionInstruction | undefined {
  const message = tx.message;
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
