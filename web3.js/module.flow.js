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
    static isPublicKey(o: {}): boolean;
    static createWithSeed(
      fromPublicKey: PublicKey,
      seed: string,
      programId: PublicKey,
    ): Promise<PublicKey>;
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
  declare export type Context = {
    slot: number,
  };

  declare export type SendOptions = {
    skipPreflight: boolean,
  };

  declare export type ConfirmOptions = {
    confirmations: number,
    skipPreflight: boolean,
  };

  declare export type RpcResponseAndContext<T> = {
    context: Context,
    value: T,
  };

  declare export type Commitment =
    | 'max'
    | 'recent'
    | 'root'
    | 'single'
    | 'singleGossip';

  declare export type LargestAccountsFilter = 'circulating' | 'nonCirculating';

  declare export type GetLargestAccountsConfig = {
    commitment: ?Commitment,
    filter: ?LargestAccountsFilter,
  };

  declare export type SignatureStatusConfig = {
    searchTransactionHistory: boolean,
  };

  declare export type SignatureStatus = {
    slot: number,
    err: TransactionError | null,
    confirmations: number | null,
  };

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
    gossip: string | null,
    tpu: string | null,
    rpc: string | null,
    version: string | null,
  };

  declare export type ConfirmedTransactionMeta = {
    fee: number,
    preBalances: Array<number>,
    postBalances: Array<number>,
    err: TransactionError | null,
  };

  declare export type ConfirmedBlock = {
    blockhash: Blockhash,
    previousBlockhash: Blockhash,
    parentSlot: number,
    transactions: Array<{
      transaction: Transaction,
      meta: ConfirmedTransactionMeta | null,
    }>,
  };

  declare export type ConfirmedTransaction = {
    slot: number,
    transaction: Transaction,
    meta: ConfirmedTransactionMeta | null,
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

  declare type AccountChangeCallback = (
    accountInfo: AccountInfo,
    context: Context,
  ) => void;
  declare type ProgramAccountChangeCallback = (
    keyedAccountInfo: KeyedAccountInfo,
    context: Context,
  ) => void;
  declare type SlotChangeCallback = (slotInfo: SlotInfo) => void;
  declare type SignatureResultCallback = (
    signatureResult: SignatureResult,
    context: Context,
  ) => void;
  declare type RootChangeCallback = (root: number) => void;

  declare export type TransactionError = {};
  declare export type SignatureResult = {|
    err: TransactionError | null,
  |};

  declare export type InflationGovernor = {
    foundation: number,
    foundationTerm: number,
    initial: number,
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

  declare export type EpochInfo = {
    epoch: number,
    slotIndex: number,
    slotsInEpoch: number,
    absoluteSlot: number,
  };

  declare export type Supply = {
    total: number,
    circulating: number,
    nonCirculating: number,
    nonCirculatingAccounts: Array<PublicKey>,
  };

  declare export type AccountBalancePair = {
    address: PublicKey,
    lamports: number,
  };

  declare export type VoteAccountStatus = {
    current: Array<VoteAccountInfo>,
    delinquent: Array<VoteAccountInfo>,
  };

  declare export class Connection {
    constructor(endpoint: string, commitment: ?Commitment): Connection;
    commitment: ?Commitment;
    getAccountInfoAndContext(
      publicKey: PublicKey,
      commitment: ?Commitment,
    ): Promise<RpcResponseAndContext<AccountInfo | null>>;
    getAccountInfo(
      publicKey: PublicKey,
      commitment: ?Commitment,
    ): Promise<AccountInfo | null>;
    getProgramAccounts(
      programId: PublicKey,
      commitment: ?Commitment,
    ): Promise<Array<PublicKeyAndAccount>>;
    getBalanceAndContext(
      publicKey: PublicKey,
      commitment: ?Commitment,
    ): Promise<RpcResponseAndContext<number>>;
    getBalance(publicKey: PublicKey, commitment: ?Commitment): Promise<number>;
    getBlockTime(slot: number): Promise<number | null>;
    getMinimumLedgerSlot(): Promise<number>;
    getFirstAvailableBlock(): Promise<number>;
    getSupply(commitment: ?Commitment): Promise<RpcResponseAndContext<Supply>>;
    getLargestAccounts(
      config: ?GetLargestAccountsConfig,
    ): Promise<RpcResponseAndContext<Array<AccountBalancePair>>>;
    getClusterNodes(): Promise<Array<ContactInfo>>;
    getConfirmedBlock(slot: number): Promise<ConfirmedBlock>;
    getConfirmedTransaction(
      signature: TransactionSignature,
    ): Promise<ConfirmedTransaction | null>;
    getConfirmedSignaturesForAddress(
      address: PublicKey,
      startSlot: number,
      endSlot: number,
    ): Promise<Array<TransactionSignature>>;
    getVoteAccounts(commitment: ?Commitment): Promise<VoteAccountStatus>;
    confirmTransaction(
      signature: TransactionSignature,
      confirmations: ?number,
    ): Promise<RpcResponseAndContext<SignatureStatus | null>>;
    getSlot(commitment: ?Commitment): Promise<number>;
    getSlotLeader(commitment: ?Commitment): Promise<string>;
    getSignatureStatus(
      signature: TransactionSignature,
      config: ?SignatureStatusConfig,
    ): Promise<RpcResponseAndContext<SignatureStatus | null>>;
    getSignatureStatuses(
      signatures: Array<TransactionSignature>,
      config: ?SignatureStatusConfig,
    ): Promise<RpcResponseAndContext<Array<SignatureStatus | null>>>;
    getTransactionCount(commitment: ?Commitment): Promise<number>;
    getTotalSupply(commitment: ?Commitment): Promise<number>;
    getVersion(): Promise<Version>;
    getInflationGovernor(commitment: ?Commitment): Promise<InflationGovernor>;
    getEpochSchedule(): Promise<EpochSchedule>;
    getEpochInfo(commitment: ?Commitment): Promise<EpochInfo>;
    getRecentBlockhashAndContext(
      commitment: ?Commitment,
    ): Promise<RpcResponseAndContext<BlockhashAndFeeCalculator>>;
    getFeeCalculatorForBlockhash(
      blockhash: Blockhash,
      commitment: ?Commitment,
    ): Promise<RpcResponseAndContext<FeeCalculator | null>>;
    getRecentBlockhash(
      commitment: ?Commitment,
    ): Promise<BlockhashAndFeeCalculator>;
    requestAirdrop(
      to: PublicKey,
      amount: number,
    ): Promise<TransactionSignature>;
    sendTransaction(
      transaction: Transaction,
      signers: Array<Account>,
      options?: SendOptions,
    ): Promise<TransactionSignature>;
    sendEncodedTransaction(
      encodedTransaction: string,
      options?: SendOptions,
    ): Promise<TransactionSignature>;
    sendRawTransaction(
      wireTransaction: Buffer | Uint8Array | Array<number>,
      options?: SendOptions,
    ): Promise<TransactionSignature>;
    onAccountChange(
      publickey: PublicKey,
      callback: AccountChangeCallback,
      commitment: ?Commitment,
    ): number;
    removeAccountChangeListener(id: number): Promise<void>;
    onProgramAccountChange(
      programId: PublicKey,
      callback: ProgramAccountChangeCallback,
      commitment: ?Commitment,
    ): number;
    removeProgramAccountChangeListener(id: number): Promise<void>;
    onSlotChange(callback: SlotChangeCallback): number;
    removeSlotChangeListener(id: number): Promise<void>;
    onSignature(
      signature: TransactionSignature,
      callback: SignatureResultCallback,
      commitment: ?Commitment,
    ): number;
    removeSignatureListener(id: number): Promise<void>;
    onRootChange(callback: RootChangeCallback): number;
    removeRootChangeListener(id: number): Promise<void>;
    validatorExit(): Promise<boolean>;
    getMinimumBalanceForRentExemption(
      dataLength: number,
      commitment: ?Commitment,
    ): Promise<number>;
    getNonce(
      nonceAccount: PublicKey,
      commitment: ?Commitment,
    ): Promise<NonceAccount>;
    getNonceAndContext(
      nonceAccount: PublicKey,
      commitment: ?Commitment,
    ): Promise<RpcResponseAndContext<NonceAccount>>;
  }

  // === src/nonce-account.js ===
  declare export class NonceAccount {
    authorizedPubkey: PublicKey;
    nonce: Blockhash;
    feeCalculator: FeeCalculator;
    static fromAccountData(
      buffer: Buffer | Uint8Array | Array<number>,
    ): NonceAccount;
  }

  declare export var NONCE_ACCOUNT_LENGTH: number;

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

  declare export function encodeData(type: InstructionType, fields: {}): Buffer;

  // === src/message.js ===
  declare export type MessageHeader = {
    numRequiredSignatures: number,
    numReadonlySignedAccounts: number,
    numReadonlyUnsignedAccounts: number,
  };

  declare export type CompiledInstruction = {
    programIdIndex: number,
    accounts: number[],
    data: string,
  };

  declare export type MessageArgs = {
    header: MessageHeader,
    accountKeys: string[],
    recentBlockhash: Blockhash,
    instructions: CompiledInstruction[],
  };

  declare export class Message {
    header: MessageHeader;
    accountKeys: PublicKey[];
    recentBlockhash: Blockhash;
    instructions: CompiledInstruction[];

    constructor(args: MessageArgs): Message;
    isAccountWritable(index: number): boolean;
    serialize(): Buffer;
  }

  // === src/transaction.js ===
  declare export type TransactionSignature = string;

  declare export type AccountMeta = {
    pubkey: PublicKey,
    isSigner: boolean,
    isWritable: boolean,
  };

  declare type TransactionInstructionCtorFields = {|
    keys: ?Array<AccountMeta>,
    programId?: PublicKey,
    data?: Buffer,
  |};

  declare export class TransactionInstruction {
    keys: Array<AccountMeta>;
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
    static populate(message: Message, signatures: Array<string>): Transaction;
    add(
      ...items: Array<
        Transaction | TransactionInstruction | TransactionInstructionCtorFields,
      >
    ): Transaction;
    compileMessage(): Message;
    serializeMessage(): Buffer;
    sign(...signers: Array<Account>): void;
    signPartial(...partialSigners: Array<PublicKey | Account>): void;
    addSigner(signer: Account): void;
    addSignature(pubkey: PublicKey, signature: Buffer): void;
    verifySignatures(): boolean;
    serialize(): Buffer;
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

  declare export type CreateStakeAccountParams = {|
    fromPubkey: PublicKey,
    stakePubkey: PublicKey,
    authorized: Authorized,
    lockup: Lockup,
    lamports: number,
  |};

  declare export type CreateStakeAccountWithSeedParams = {|
    fromPubkey: PublicKey,
    stakePubkey: PublicKey,
    basePubkey: PublicKey,
    seed: string,
    authorized: Authorized,
    lockup: Lockup,
    lamports: number,
  |};

  declare export type InitializeStakeParams = {|
    stakePubkey: PublicKey,
    authorized: Authorized,
    lockup: Lockup,
  |};

  declare export type DelegateStakeParams = {|
    stakePubkey: PublicKey,
    authorizedPubkey: PublicKey,
    votePubkey: PublicKey,
  |};

  declare export type AuthorizeStakeParams = {|
    stakePubkey: PublicKey,
    authorizedPubkey: PublicKey,
    newAuthorizedPubkey: PublicKey,
    stakeAuthorizationType: StakeAuthorizationType,
  |};

  declare export type SplitStakeParams = {|
    stakePubkey: PublicKey,
    authorizedPubkey: PublicKey,
    splitStakePubkey: PublicKey,
    lamports: number,
  |};

  declare export type WithdrawStakeParams = {|
    stakePubkey: PublicKey,
    authorizedPubkey: PublicKey,
    toPubkey: PublicKey,
    lamports: number,
  |};

  declare export type DeactivateStakeParams = {|
    stakePubkey: PublicKey,
    authorizedPubkey: PublicKey,
  |};

  declare export class StakeProgram {
    static programId: PublicKey;
    static space: number;
    static createAccount(params: CreateStakeAccountParams): Transaction;
    static createAccountWithSeed(
      params: CreateStakeAccountWithSeedParams,
    ): Transaction;
    static delegate(params: DelegateStakeParams): Transaction;
    static authorize(params: AuthorizeStakeParams): Transaction;
    static split(params: SplitStakeParams): Transaction;
    static withdraw(params: WithdrawStakeParams): Transaction;
    static deactivate(params: DeactivateStakeParams): Transaction;
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

  declare export class StakeInstruction {
    static decodeInstructionType(
      instruction: TransactionInstruction,
    ): StakeInstructionType;
    static decodeInitialize(
      instruction: TransactionInstruction,
    ): InitializeStakeParams;
    static decodeDelegate(
      instruction: TransactionInstruction,
    ): DelegateStakeParams;
    static decodeAuthorize(
      instruction: TransactionInstruction,
    ): AuthorizeStakeParams;
    static decodeSplit(instruction: TransactionInstruction): SplitStakeParams;
    static decodeWithdraw(
      instruction: TransactionInstruction,
    ): WithdrawStakeParams;
    static decodeDeactivate(
      instruction: TransactionInstruction,
    ): DeactivateStakeParams;
  }

  // === src/system-program.js ===
  declare export type CreateAccountParams = {|
    fromPubkey: PublicKey,
    newAccountPubkey: PublicKey,
    lamports: number,
    space: number,
    programId: PublicKey,
  |};

  declare export type CreateAccountWithSeedParams = {|
    fromPubkey: PublicKey,
    newAccountPubkey: PublicKey,
    basePubkey: PublicKey,
    seed: string,
    lamports: number,
    space: number,
    programId: PublicKey,
  |};

  declare export type AllocateParams = {|
    accountPubkey: PublicKey,
    space: number,
  |};

  declare export type AllocateWithSeedParams = {|
    accountPubkey: PublicKey,
    basePubkey: PublicKey,
    seed: string,
    space: number,
    programId: PublicKey,
  |};

  declare export type AssignParams = {|
    accountPubkey: PublicKey,
    programId: PublicKey,
  |};

  declare export type AssignWithSeedParams = {|
    accountPubkey: PublicKey,
    basePubkey: PublicKey,
    seed: string,
    programId: PublicKey,
  |};

  declare export type TransferParams = {|
    fromPubkey: PublicKey,
    toPubkey: PublicKey,
    lamports: number,
  |};

  declare export type CreateNonceAccountParams = {|
    fromPubkey: PublicKey,
    noncePubkey: PublicKey,
    authorizedPubkey: PublicKey,
    lamports: number,
  |};

  declare export type CreateNonceAccountWithSeedParams = {|
    fromPubkey: PublicKey,
    noncePubkey: PublicKey,
    authorizedPubkey: PublicKey,
    lamports: number,
    basePubkey: PublicKey,
    seed: string,
  |};

  declare export type InitializeNonceParams = {|
    noncePubkey: PublicKey,
    authorizedPubkey: PublicKey,
  |};

  declare export type AdvanceNonceParams = {|
    noncePubkey: PublicKey,
    authorizedPubkey: PublicKey,
  |};

  declare export type WithdrawNonceParams = {|
    noncePubkey: PublicKey,
    authorizedPubkey: PublicKey,
    toPubkey: PublicKey,
    lamports: number,
  |};

  declare export type AuthorizeNonceParams = {|
    noncePubkey: PublicKey,
    authorizedPubkey: PublicKey,
    newAuthorizedPubkey: PublicKey,
  |};

  declare export class SystemProgram {
    static programId: PublicKey;

    static createAccount(params: CreateAccountParams): Transaction;
    static createAccountWithSeed(
      params: CreateAccountWithSeedParams,
    ): Transaction;
    static allocate(
      params: AllocateParams | AllocateWithSeedParams,
    ): Transaction;
    static assign(params: AssignParams | AssignWithSeedParams): Transaction;
    static transfer(params: TransferParams): Transaction;
    static createNonceAccount(
      params: CreateNonceAccountParams | CreateNonceAccountWithSeedParams,
    ): Transaction;
    static nonceAdvance(params: AdvanceNonceParams): TransactionInstruction;
    static nonceWithdraw(params: WithdrawNonceParams): Transaction;
    static nonceAuthorize(params: AuthorizeNonceParams): Transaction;
  }

  declare export type SystemInstructionType =
    | 'Create'
    | 'CreateWithSeed'
    | 'Allocate'
    | 'AllocateWithSeed'
    | 'Assign'
    | 'AssignWithSeed'
    | 'Transfer'
    | 'AdvanceNonceAccount'
    | 'WithdrawNonceAccount'
    | 'InitializeNonceAccount'
    | 'AuthorizeNonceAccount';

  declare export var SYSTEM_INSTRUCTION_LAYOUTS: {
    [SystemInstructionType]: InstructionType,
  };

  declare export class SystemInstruction {
    static decodeInstructionType(
      instruction: TransactionInstruction,
    ): SystemInstructionType;
    static decodeCreateAccount(
      instruction: TransactionInstruction,
    ): CreateAccountParams;
    static decodeCreateWithSeed(
      instruction: TransactionInstruction,
    ): CreateAccountWithSeedParams;
    static decodeAllocate(instruction: TransactionInstruction): AllocateParams;
    static decodeAllocateWithSeed(
      instruction: TransactionInstruction,
    ): AllocateWithSeedParams;
    static decodeAssign(instruction: TransactionInstruction): AssignParams;
    static decodeAssignWithSeed(
      instruction: TransactionInstruction,
    ): AssignWithSeedParams;
    static decodeTransfer(instruction: TransactionInstruction): TransferParams;
    static decodeNonceInitialize(
      instruction: TransactionInstruction,
    ): InitializeNonceParams;
    static decodeNonceAdvance(
      instruction: TransactionInstruction,
    ): AdvanceNonceParams;
    static decodeNonceWithdraw(
      instruction: TransactionInstruction,
    ): WithdrawNonceParams;
    static decodeNonceAuthorize(
      instruction: TransactionInstruction,
    ): AuthorizeNonceParams;
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
      program: Account,
      elfBytes: Buffer | Uint8Array | Array<number>,
    ): Promise<PublicKey>;
  }

  // === src/util/send-and-confirm-transaction.js ===
  declare export function sendAndConfirmTransaction(
    connection: Connection,
    transaction: Transaction,
    signers: Array<Account>,
    options: ?ConfirmOptions,
  ): Promise<TransactionSignature>;

  // === src/util/send-and-confirm-raw-transaction.js ===
  declare export function sendAndConfirmRawTransaction(
    connection: Connection,
    wireTransaction: Buffer,
    options: ?ConfirmOptions,
  ): Promise<TransactionSignature>;

  // === src/util/cluster.js ===
  declare export type Cluster = 'devnet' | 'testnet' | 'mainnet-beta';

  declare export function clusterApiUrl(
    cluster?: Cluster,
    tls?: boolean,
  ): string;

  // === src/index.js ===
  declare export var LAMPORTS_PER_SOL: number;
}
