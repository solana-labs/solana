/**
 * Flow Library definition for @solana/web3.js
 *
 * This file is manually generated from the contents of src/
 *
 * Usage: add the following line under the [libs] section of your project's
 * .flowconfig:
 * [libs]
 * node_modules/@solana/web3.js/module.flow.js
 *
 */

import BN from 'bn.js';

declare module '@solana/web3.js' {
  // === src/publickey.js ===
  declare export class PublicKey {
    constructor(value: number | string | Buffer | Array<number>): PublicKey;
    static isPublicKey(o: Object): boolean;
    equals(publickey: PublicKey): boolean;
    toBase58(): string;
    toBuffer(): Buffer;
    toString(): string;
  }

  // === src/blockhash.js ===
  declare export type Blockhash = string;

  // === src/account.js ===
  declare export class Account {
    constructor(secretKey: ?Buffer): Account;
    publicKey: PublicKey;
    secretKey: Buffer;
  }

  // === src/fee-calculator.js ===
  declare export type FeeCalculator = {
    burnPercent: number,
    lamportsPerSignature: number,
    maxLamportsPerSignature: number,
    minLamportsPerSignature: number,
    targetLamportsPerSignature: number,
    targetSignaturesPerSlot: number,
  };

  // === src/budget-program.js ===
  /* TODO */

  // === src/connection.js ===
  declare export type RpcResponseAndContext<T> = {
    context: {
      slot: number,
    },
    value: T,
  };

  declare export type Commitment = 'max' | 'recent';

  declare export type AccountInfo = {
    executable: boolean,
    owner: PublicKey,
    lamports: number,
    data: Buffer,
    rent_epoch: number | null,
  };

  declare export type ContactInfo = {
    id: string,
    gossip: string,
    tpu: string | null,
    rpc: string | null,
  };

  declare export type ConfirmedBlock = {
    blockhash: Blockhash,
    previousBlockhash: Blockhash,
    parentSlot: number,
    transactions: Array<[Transaction, SignatureSuccess | TransactionError | null]>,
  };

  declare export type KeyedAccountInfo = {
    accountId: PublicKey,
    accountInfo: AccountInfo,
  };

  declare export type Version = {
    'solana-core': string,
  };

  declare export type VoteAccountInfo = {
    votePubkey: string,
    nodePubkey: string,
    stake: number,
    commission: number,
  };

  declare type AccountChangeCallback = (accountInfo: AccountInfo) => void;
  declare type ProgramAccountChangeCallback = (
    keyedAccountInfo: KeyedAccountInfo,
  ) => void;

  declare export type SignatureSuccess = {|
    Ok: null,
  |};
  declare export type TransactionError = {|
    Err: Object,
  |};

  declare export type Inflation = {
    foundation: number,
    foundation_term: number,
    initial: number,
    storage: number,
    taper: number,
    terminal: number,
  };

  declare export type EpochSchedule = {
    slots_per_epoch: number,
    leader_schedule_slot_offset: number,
    warmup: boolean,
    first_normal_epoch: number,
    first_normal_slot: number,
  };

  declare export class Connection {
    constructor(endpoint: string, commitment: ?Commitment): Connection;
    getAccountInfoAndContext(
      publicKey: PublicKey,
      commitment: ?Commitment,
    ): Promise<RpcResponseAndContext<AccountInfo>>;
    getAccountInfo(
      publicKey: PublicKey,
      commitment: ?Commitment,
    ): Promise<AccountInfo>;
    getProgramAccounts(
      programId: PublicKey,
      commitment: ?Commitment,
    ): Promise<Array<[PublicKey, AccountInfo]>>;
    getBalanceAndContext(
      publicKey: PublicKey,
      commitment: ?Commitment,
    ): Promise<RpcResponseAndContext<number>>;
    getBalance(publicKey: PublicKey, commitment: ?Commitment): Promise<number>;
    getClusterNodes(): Promise<Array<ContactInfo>>;
    getConfirmedBlock(): Promise<ConfirmedBlock>;
    getVoteAccounts(commitment: ?Commitment): Promise<VoteAccountStatus>;
    confirmTransactionAndContext(
      signature: TransactionSignature,
      commitment: ?Commitment,
    ): Promise<RpcResponseAndContext<boolean>>;
    confirmTransaction(
      signature: TransactionSignature,
      commitment: ?Commitment,
    ): Promise<boolean>;
    getSlot(commitment: ?Commitment): Promise<number>;
    getSlotLeader(commitment: ?Commitment): Promise<string>;
    getSignatureStatus(
      signature: TransactionSignature,
      commitment: ?Commitment,
    ): Promise<SignatureSuccess | TransactionError | null>;
    getTransactionCount(commitment: ?Commitment): Promise<number>;
    getTotalSupply(commitment: ?Commitment): Promise<number>;
    getVersion(): Promise<Version>;
    getInflation(commitment: ?Commitment): Promise<Inflation>;
    getEpochSchedule(): Promise<EpochSchedule>;
    getRecentBlockhashAndContext(
      commitment: ?Commitment,
    ): Promise<RpcResponseAndContext<[Blockhash, FeeCalculator]>>;
    getRecentBlockhash(
      commitment: ?Commitment,
    ): Promise<[Blockhash, FeeCalculator]>;
    requestAirdrop(
      to: PublicKey,
      amount: number,
      commitment: ?Commitment,
    ): Promise<TransactionSignature>;
    sendTransaction(
      transaction: Transaction,
      ...signers: Array<Account>
    ): Promise<TransactionSignature>;
    sendRawTransaction(wireTransaction: Buffer): Promise<TransactionSignature>;
    onAccountChange(
      publickey: PublicKey,
      callback: AccountChangeCallback,
    ): number;
    removeAccountChangeListener(id: number): Promise<void>;
    onProgramAccountChange(
      programId: PublicKey,
      callback: ProgramAccountChangeCallback,
    ): number;
    removeProgramAccountChangeListener(id: number): Promise<void>;
    validatorExit(): Promise<boolean>;
  }

  // === src/system-program.js ===
  declare export class SystemProgram {
    static programId: PublicKey;

    static createAccount(
      from: PublicKey,
      newAccount: PublicKey,
      lamports: number,
      space: number,
      programId: PublicKey,
    ): Transaction;
    static transfer(
      from: PublicKey,
      to: PublicKey,
      amount: number,
    ): Transaction;
    static assign(from: PublicKey, programId: PublicKey): Transaction;
  }

  // === src/validator-info.js ===
  declare export var VALIDATOR_INFO_KEY;
  declare export type Info = {|
    name: string,
    website?: string,
    details?: string,
    keybaseUsername?: string,
  |};

  declare export class ValidatorInfo {
    key: PublicKey;
    info: Info;

    constructor(key: PublicKey, info: Info): ValidatorInfo;
    static fromConfigData(buffer: Buffer): ?ValidatorInfo;
  }

  // === src/sysvar-rent.js ===
  declare export var SYSVAR_RENT_PUBKEY;

  // === src/vote-account.js ===
  declare export var VOTE_ACCOUNT_KEY;
  declare export type Lockout = {|
    slot: number,
    confirmationCount: number,
  |};

  declare export type EpochCredits = {|
    epoch: number,
    credits: number,
    prevCredits: number,
  |};

  declare export class VoteAccount {
    votes: Array<Lockout>;
    nodePubkey: PublicKey;
    authorizedVoterPubkey: PublicKey;
    commission: number;
    rootSlot: number | null;
    epoch: number;
    credits: number;
    lastEpochCredits: number;
    epochCredits: Array<EpochCredits>;
    static fromAccountData(buffer: Buffer): VoteAccount;
  }

  // === src/transaction.js ===
  declare export type TransactionSignature = string;

  declare type TransactionInstructionCtorFields = {|
    keys: ?Array<{pubkey: PublicKey, isSigner: boolean, isWritable: boolean}>,
    programId?: PublicKey,
    data?: Buffer,
  |};

  declare export class TransactionInstruction {
    keys: Array<{pubkey: PublicKey, isSigner: boolean, isWritable: boolean}>;
    programId: PublicKey;
    data: Buffer;

    constructor(
      opts?: TransactionInstructionCtorFields,
    ): TransactionInstruction;
  }

  declare type SignaturePubkeyPair = {|
    signature: Buffer | null,
    publicKey: PublicKey,
  |};

  declare type TransactionCtorFields = {|
    recentBlockhash?: Blockhash,
    signatures?: Array<SignaturePubkeyPair>,
  |};

  declare export class Transaction {
    signatures: Array<SignaturePubkeyPair>;
    signature: ?Buffer;
    instructions: Array<TransactionInstruction>;
    recentBlockhash: ?Blockhash;

    constructor(opts?: TransactionCtorFields): Transaction;
    add(
      ...items: Array<
        Transaction | TransactionInstruction | TransactionInstructionCtorFields,
      >
    ): Transaction;
    sign(...signers: Array<Account>): void;
    signPartial(...partialSigners: Array<PublicKey | Account>): void;
    addSigner(signer: Account): void;
    serialize(): Buffer;
  }

  // === src/loader.js ===
  declare export class Loader {
    static getMinNumSignatures(dataLength: number): number;
    static load(
      connection: Connection,
      payer: Account,
      program: Account,
      programId: PublicKey,
      data: Array<number>,
    ): Promise<PublicKey>;
  }

  // === src/bpf-loader.js ===
  declare export class BpfLoader {
    static programId: PublicKey;
    static getMinNumSignatures(dataLength: number): number;
    static load(
      connection: Connection,
      payer: Account,
      elfBytes: Array<number>,
    ): Promise<PublicKey>;
  }

  // === src/util/send-and-confirm-transaction.js ===
  declare export function sendAndConfirmTransaction(
    connection: Connection,
    transaction: Transaction,
    ...signers: Array<Account>
  ): Promise<TransactionSignature>;

  declare export function sendAndConfirmRecentTransaction(
    connection: Connection,
    transaction: Transaction,
    ...signers: Array<Account>
  ): Promise<TransactionSignature>;

  // === src/util/send-and-confirm-raw-transaction.js ===
  declare export function sendAndConfirmRawTransaction(
    connection: Connection,
    wireTransaction: Buffer,
    commitment: ?Commitment,
  ): Promise<TransactionSignature>;

  // === src/util/testnet.js ===
  declare export function testnetChannelEndpoint(channel?: string): string;

  declare export var SOL_LAMPORTS: number;
}
