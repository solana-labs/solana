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
    constructor(number: string | Buffer | Array<number>): PublicKey;
    static isPublicKey(o: Object): boolean;
    equals(publickey: PublicKey): boolean;
    toBase58(): string;
    toBuffer(): Buffer;
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

  declare export type KeyedAccountInfo = {
    accountId: PublicKey,
    accountInfo: AccountInfo,
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

  declare export class Connection {
    constructor(endpoint: string): Connection;
    getAccountInfo(publicKey: PublicKey): Promise<AccountInfo>;
    getProgramAccounts(
      programId: PublicKey,
    ): Promise<Array<[PublicKey, AccountInfo]>>;
    getBalance(publicKey: PublicKey): Promise<number>;
    getClusterNodes(): Promise<Array<ContactInfo>>;
    getVoteAccounts(): Promise<VoteAccountStatus>;
    confirmTransaction(signature: TransactionSignature): Promise<boolean>;
    getSlot(): Promise<number>;
    getSlotLeader(): Promise<string>;
    getSignatureStatus(
      signature: TransactionSignature,
    ): Promise<SignatureSuccess | TransactionError | null>;
    getTransactionCount(): Promise<number>;
    getTotalSupply(): Promise<number>;
    getRecentBlockhash(): Promise<[Blockhash, FeeCalculator]>;
    requestAirdrop(
      to: PublicKey,
      amount: number,
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
    fullnodeExit(): Promise<boolean>;
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
    keys: ?Array<{pubkey: PublicKey, isSigner: boolean, isDebitable: boolean}>,
    programId?: PublicKey,
    data?: Buffer,
  |};

  declare export class TransactionInstruction {
    keys: Array<{pubkey: PublicKey, isSigner: boolean, isDebitable: boolean}>;
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
      ...items: Array<Transaction | TransactionInstructionCtorFields>
    ): Transaction;
    sign(...signers: Array<Account>): void;
    signPartial(...partialSigners: Array<PublicKey | Account>): void;
    addSigner(signer: Account): void;
    serialize(): Buffer;
  }

  // === src/token-program.js ===
  declare export class TokenAmount extends BN {
    toBuffer(): Buffer;
    fromBuffer(buffer: Buffer): TokenAmount;
  }

  declare export type TokenInfo = {|
    supply: TokenAmount,
    decimals: number,
    name: string,
    symbol: string,
  |};
  declare export type TokenAccountInfo = {|
    token: PublicKey,
    owner: PublicKey,
    amount: TokenAmount,
    source: null | PublicKey,
    originalAmount: TokenAmount,
  |};
  declare type TokenAndPublicKey = [Token, PublicKey];

  declare export class Token {
    programId: PublicKey;
    token: PublicKey;

    static createNewToken(
      connection: Connection,
      owner: Account,
      supply: TokenAmount,
      name: string,
      symbol: string,
      decimals: number,
      programId?: PublicKey,
    ): Promise<TokenAndPublicKey>;

    constructor(connection: Connection, token: PublicKey): Token;
    newAccount(owner: Account, source?: PublicKey): Promise<PublicKey>;
    tokenInfo(): Promise<TokenInfo>;
    accountInfo(account: PublicKey): Promise<TokenAccountInfo>;
    transfer(
      owner: Account,
      source: PublicKey,
      destination: PublicKey,
      amount: number | TokenAmount,
    ): Promise<TransactionSignature>;
    approve(
      owner: Account,
      account: PublicKey,
      delegate: PublicKey,
      amount: number | TokenAmount,
    ): Promise<void>;
    revoke(
      owner: Account,
      account: PublicKey,
      delegate: PublicKey,
    ): Promise<void>;
    setOwner(
      owner: Account,
      account: PublicKey,
      newOwner: PublicKey,
    ): Promise<void>;

    transferInstruction(
      owner: PublicKey,
      source: PublicKey,
      destination: PublicKey,
      amount: number | TokenAmount,
    ): Promise<TransactionInstruction>;
    approveInstruction(
      owner: PublicKey,
      account: PublicKey,
      delegate: PublicKey,
      amount: number | TokenAmount,
    ): TransactionInstruction;
    revokeInstruction(
      owner: PublicKey,
      account: PublicKey,
      delegate: PublicKey,
    ): TransactionInstruction;
    setOwnerInstruction(
      owner: PublicKey,
      account: PublicKey,
      newOwner: PublicKey,
    ): TransactionInstruction;
  }

  // === src/loader.js ===
  declare export class Loader {
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

  // === src/util/send-and-confirm-raw-transaction.js ===
  declare export function sendAndConfirmRawTransaction(
    connection: Connection,
    wireTransaction: Buffer,
  ): Promise<TransactionSignature>;

  // === src/util/testnet.js ===
  declare export function testnetChannelEndpoint(channel?: string): string;
}
