/**
 * Flow Library definition for @solana/web3.js
 *
 * This file is manually maintained
 *
 * Usage: add the following line under the [libs] section of your project's
 * .flowconfig:
 * [libs]
 * node_modules/@solana/web3.js/module.flow.js
 *
 */

import {Buffer} from 'buffer';
import * as BufferLayout from 'buffer-layout';

declare module '@solana/web3.js' {
  // === src/publickey.js ===
  declare export class PublicKey {
    constructor(
      value: number | string | Buffer | Uint8Array | Array<number>,
    ): PublicKey;
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
    constructor(secretKey?: Buffer | Uint8Array | Array<number>): Account;
    publicKey: PublicKey;
    secretKey: Buffer;
  }

  // === src/fee-calculator.js ===
  declare export type FeeCalculator = {
    lamportsPerSignature: number,
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

  declare export type SignatureStatusResult =
    | SignatureSuccess
    | TransactionError;

  declare export type BlockhashAndFeeCalculator = {
    blockhash: Blockhash,
    feeCalculator: FeeCalculator,
  };

  declare export type PublicKeyAndAccount = {
    pubkey: PublicKey,
    account: AccountInfo,
  };

  declare export type AccountInfo = {
    executable: boolean,
    owner: PublicKey,
    lamports: number,
    data: Buffer,
    rentEpoch: number | null,
  };

  declare export type ContactInfo = {
    pubkey: string,
    gossip: string,
    tpu: string | null,
    rpc: string | null,
  };

  declare export type ConfirmedBlock = {
    blockhash: Blockhash,
    previousBlockhash: Blockhash,
    parentSlot: number,
    transactions: Array<{
      transaction: Transaction,
      meta: {
        fee: number,
        preBalances: Array<number>,
        postBalances: Array<number>,
        status: ?SignatureStatusResult,
      },
    }>,
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

  declare export type SlotInfo = {
    parent: 'number',
    slot: 'number',
    root: 'number',
  };

  declare type AccountChangeCallback = (accountInfo: AccountInfo) => void;
  declare type ProgramAccountChangeCallback = (
    keyedAccountInfo: KeyedAccountInfo,
  ) => void;
  declare type SlotChangeCallback = (slotInfo: SlotInfo) => void;
  declare type SignatureResultCallback = (
    signatureResult: SignatureStatusResult,
  ) => void;

  declare export type SignatureSuccess = {|
    Ok: null,
  |};
  declare export type TransactionError = {|
    Err: Object,
  |};

  declare export type Inflation = {
    foundation: number,
    foundationTerm: number,
    initial: number,
    storage: number,
    taper: number,
    terminal: number,
  };

  declare export type EpochSchedule = {
    slotsPerEpoch: number,
    leaderScheduleSlotOffset: number,
    warmup: boolean,
    firstNormalEpoch: number,
    firstNormalSlot: number,
  };

  declare export type VoteAccountStatus = {
    current: Array<VoteAccountInfo>,
    delinquent: Array<VoteAccountInfo>,
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
    ): Promise<Array<PublicKeyAndAccount>>;
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
    ): Promise<RpcResponseAndContext<BlockhashAndFeeCalculator>>;
    getRecentBlockhash(
      commitment: ?Commitment,
    ): Promise<BlockhashAndFeeCalculator>;
    requestAirdrop(
      to: PublicKey,
      amount: number,
      commitment: ?Commitment,
    ): Promise<TransactionSignature>;
    sendTransaction(
      transaction: Transaction,
      ...signers: Array<Account>
    ): Promise<TransactionSignature>;
    sendEncodedTransaction(
      encodedTransaction: string,
    ): Promise<TransactionSignature>;
    sendRawTransaction(
      wireTransaction: Buffer | Uint8Array | Array<number>,
    ): Promise<TransactionSignature>;
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
    onSlotChange(callback: SlotChangeCallback): number;
    removeSlotChangeListener(id: number): Promise<void>;
    onSignature(
      signature: TransactionSignature,
      callback: SignatureResultCallback,
    ): number;
    removeSignatureListener(id: number): Promise<void>;
    validatorExit(): Promise<boolean>;
    getMinimumBalanceForRentExemption(
      dataLength: number,
      commitment: ?Commitment,
    ): Promise<number>;
  }

  // === src/stake-program.js ===
  declare export type StakeAuthorizationType = {|
    index: number,
  |};

  declare export class Authorized {
    staker: PublicKey;
    withdrawer: PublicKey;
    constructor(staker: PublicKey, withdrawer: PublicKey): Authorized;
  }

  declare export class Lockup {
    unixTimestamp: number;
    epoch: number;
    custodian: PublicKey;
    constructor(
      unixTimestamp: number,
      epoch: number,
      custodian: PublicKey,
    ): Lockup;
  }

  declare export class StakeProgram {
    static programId: PublicKey;
    static space: number;

    static createAccount(
      from: PublicKey,
      stakeAccount: PublicKey,
      authorized: Authorized,
      lockup: Lockup,
      lamports: number,
    ): Transaction;
    static createAccountWithSeed(
      from: PublicKey,
      stakeAccount: PublicKey,
      seed: string,
      authorized: Authorized,
      lockup: Lockup,
      lamports: number,
    ): Transaction;
    static delegate(
      stakeAccount: PublicKey,
      authorizedPubkey: PublicKey,
      votePubkey: PublicKey,
    ): Transaction;
    static authorize(
      stakeAccount: PublicKey,
      authorizedPubkey: PublicKey,
      newAuthorized: PublicKey,
      stakeAuthorizationType: StakeAuthorizationType,
    ): Transaction;
    static split(
      stakeAccount: PublicKey,
      authorizedPubkey: PublicKey,
      lamports: number,
      splitStakePubkey: PublicKey,
    ): Transaction;
    static withdraw(
      stakeAccount: PublicKey,
      withdrawerPubkey: PublicKey,
      to: PublicKey,
      lamports: number,
    ): Transaction;
    static deactivate(
      stakeAccount: PublicKey,
      authorizedPubkey: PublicKey,
    ): Transaction;
  }

  declare export type StakeInstructionType =
    | 'Initialize'
    | 'Authorize'
    | 'Delegate'
    | 'Split'
    | 'Withdraw'
    | 'Deactivate';

  declare export var STAKE_INSTRUCTION_LAYOUTS: {
    [StakeInstructionType]: InstructionType,
  };

  declare export class StakeInstruction extends TransactionInstruction {
    type: StakeInstructionType;
    stakePublicKey: PublicKey | null;
    authorizedPublicKey: PublicKey | null;

    constructor(
      opts?: TransactionInstructionCtorFields,
      type: StakeInstructionType,
    ): StakeInstruction;
    static from(instruction: TransactionInstruction): StakeInstruction;
  }

  // === src/system-program.js ===
  declare export class SystemProgram {
    static programId: PublicKey;
    static nonceSpace: number;

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
    static createAccountWithSeed(
      from: PublicKey,
      newAccount: PublicKey,
      base: PublicKey,
      seed: string,
      lamports: number,
      space: number,
      programId: PublicKey,
    ): Transaction;
    static createNonceAccount(
      from: PublicKey,
      nonceAccount: PublicKey,
      authorizedPubkey: PublicKey,
      lamports: number,
    ): Transaction;
    static nonceAdvance(
      nonceAccount: PublicKey,
      authorizedPubkey: PublicKey,
    ): TransactionInstruction;
    static nonceWithdraw(
      nonceAccount: PublicKey,
      authorizedPubkey: PublicKey,
      to: PublicKey,
      lamports: number,
    ): Transaction;
    static nonceAuthorize(
      nonceAccount: PublicKey,
      authorizedPubkey: PublicKey,
      newAuthorized: PublicKey,
    ): Transaction;
  }

  declare export type SystemInstructionType =
    | 'Create'
    | 'Assign'
    | 'Transfer'
    | 'CreateWithSeed'
    | 'AdvanceNonceAccount'
    | 'WithdrawNonceAccount'
    | 'InitializeNonceAccount'
    | 'AuthorizeNonceAccount';

  declare export var SYSTEM_INSTRUCTION_LAYOUTS: {
    [SystemInstructionType]: InstructionType,
  };

  declare export class SystemInstruction extends TransactionInstruction {
    type: SystemInstructionType;
    fromPublicKey: PublicKey | null;
    toPublicKey: PublicKey | null;
    amount: number | null;

    constructor(
      opts?: TransactionInstructionCtorFields,
      type: SystemInstructionType,
    ): SystemInstruction;
    static from(instruction: TransactionInstruction): SystemInstruction;
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
    static fromConfigData(
      buffer: Buffer | Uint8Array | Array<number>,
    ): ValidatorInfo | null;
  }

  // === src/sysvar.js ===
  declare export var SYSVAR_CLOCK_PUBKEY;
  declare export var SYSVAR_RENT_PUBKEY;
  declare export var SYSVAR_REWARDS_PUBKEY;
  declare export var SYSVAR_STAKE_HISTORY_PUBKEY;

  // === src/vote-account.js ===
  declare export var VOTE_PROGRAM_ID;
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
    static fromAccountData(
      buffer: Buffer | Uint8Array | Array<number>,
    ): VoteAccount;
  }

  // === src/instruction.js ===
  declare export type InstructionType = {|
    index: number,
    layout: typeof BufferLayout,
  |};

  declare export function encodeData(
    type: InstructionType,
    fields: Object,
  ): Buffer;

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

  declare type NonceInformation = {|
    nonce: Blockhash,
    nonceInstruction: TransactionInstruction,
  |};

  declare type TransactionCtorFields = {|
    recentBlockhash?: Blockhash,
    nonceInfo?: NonceInformation,
    signatures?: Array<SignaturePubkeyPair>,
  |};

  declare export class Transaction {
    signatures: Array<SignaturePubkeyPair>;
    signature: ?Buffer;
    instructions: Array<TransactionInstruction>;
    recentBlockhash: ?Blockhash;
    nonceInfo: ?NonceInformation;

    constructor(opts?: TransactionCtorFields): Transaction;
    static from(buffer: Buffer | Uint8Array | Array<number>): Transaction;
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
      data: Buffer | Uint8Array | Array<number>,
    ): Promise<PublicKey>;
  }

  // === src/bpf-loader.js ===
  declare export class BpfLoader {
    static programId: PublicKey;
    static getMinNumSignatures(dataLength: number): number;
    static load(
      connection: Connection,
      payer: Account,
      elfBytes: Buffer | Uint8Array | Array<number>,
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
  declare export function testnetChannelEndpoint(
    channel?: string,
    tls?: boolean,
  ): string;

  declare export var LAMPORTS_PER_SOL: number;
}
