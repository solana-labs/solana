declare module '@solana/web3.js' {
  import {Buffer} from 'buffer';
  import * as BufferLayout from 'buffer-layout';

  // === src/publickey.js ===
  export class PublicKey {
    constructor(value: number | string | Buffer | Uint8Array | Array<number>);
    static isPublicKey(o: object): boolean;
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
  export type Blockhash = string;

  // === src/account.js ===
  export class Account {
    constructor(secretKey?: Buffer | Uint8Array | Array<number>);
    publicKey: PublicKey;
    secretKey: Buffer;
  }

  // === src/fee-calculator.js ===
  export type FeeCalculator = {
    lamportsPerSignature: number;
  };

  // === src/budget-program.js ===

  /* TODO */

  // === src/connection.js ===
  export type Context = {
    slot: number;
  };

  export type RpcResponseAndContext<T> = {
    context: Context;
    value: T;
  };

  export type Commitment = 'max' | 'recent' | 'root' | 'single';

  export type SignatureStatusConfig = {
    searchTransactionHistory: boolean;
  };

  export type SignatureStatus = {
    slot: number;
    err: TransactionError | null;
    confirmations: number | null;
  };

  export type BlockhashAndFeeCalculator = {
    blockhash: Blockhash;
    feeCalculator: FeeCalculator;
  };

  export type PublicKeyAndAccount = {
    pubkey: PublicKey;
    account: AccountInfo;
  };

  export type AccountInfo = {
    executable: boolean;
    owner: PublicKey;
    lamports: number;
    data: Buffer;
    rentEpoch?: number;
  };

  export type ContactInfo = {
    pubkey: string;
    gossip?: string;
    tpu?: string;
    rpc?: string;
    version?: string;
  };

  export type ConfirmedTransactionMeta = {
    fee: number;
    preBalances: Array<number>;
    postBalances: Array<number>;
    err: TransactionError | null;
  };

  export type ConfirmedBlock = {
    blockhash: Blockhash;
    previousBlockhash: Blockhash;
    parentSlot: number;
    transactions: Array<{
      transaction: Transaction;
      meta: ConfirmedTransactionMeta | null;
    }>;
  };

  export type ConfirmedTransaction = {
    slot: number;
    transaction: Transaction;
    meta: ConfirmedTransactionMeta | null;
  };

  export type KeyedAccountInfo = {
    accountId: PublicKey;
    accountInfo: AccountInfo;
  };

  export type Version = {
    'solana-core': string;
  };

  export type VoteAccountInfo = {
    votePubkey: string;
    nodePubkey: string;
    stake: number;
    commission: number;
  };

  export type SlotInfo = {
    parent: 'number';
    slot: 'number';
    root: 'number';
  };

  export type AccountChangeCallback = (
    accountInfo: AccountInfo,
    context: Context,
  ) => void;
  export type ProgramAccountChangeCallback = (
    keyedAccountInfo: KeyedAccountInfo,
    context: Context,
  ) => void;
  export type SlotChangeCallback = (slotInfo: SlotInfo) => void;
  export type SignatureResultCallback = (
    signatureResult: SignatureResult,
    context: Context,
  ) => void;
  export type RootChangeCallback = (root: number) => void;

  export type TransactionError = object;
  export type SignatureResult = {
    err: TransactionError | null;
  };

  export type Inflation = {
    foundation: number;
    foundationTerm: number;
    initial: number;
    storage: number;
    taper: number;
    terminal: number;
  };

  export type EpochInfo = {
    epoch: number;
    slotIndex: number;
    slotsInEpoch: number;
    absoluteSlot: number;
  };

  export type EpochSchedule = {
    slotsPerEpoch: number;
    leaderScheduleSlotOffset: number;
    warmup: boolean;
    firstNormalEpoch: number;
    firstNormalSlot: number;
  };

  export type VoteAccountStatus = {
    current: Array<VoteAccountInfo>;
    delinquent: Array<VoteAccountInfo>;
  };

  export class Connection {
    constructor(endpoint: string, commitment?: Commitment);
    commitment?: Commitment;
    getAccountInfoAndContext(
      publicKey: PublicKey,
      commitment?: Commitment,
    ): Promise<RpcResponseAndContext<AccountInfo | null>>;
    getAccountInfo(
      publicKey: PublicKey,
      commitment?: Commitment,
    ): Promise<AccountInfo | null>;
    getProgramAccounts(
      programId: PublicKey,
      commitment?: Commitment,
    ): Promise<Array<PublicKeyAndAccount>>;
    getBalanceAndContext(
      publicKey: PublicKey,
      commitment?: Commitment,
    ): Promise<RpcResponseAndContext<number>>;
    getBalance(publicKey: PublicKey, commitment?: Commitment): Promise<number>;
    getBlockTime(slot: number): Promise<number | null>;
    getMinimumLedgerSlot(): Promise<number>;
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
    getVoteAccounts(commitment?: Commitment): Promise<VoteAccountStatus>;
    confirmTransaction(
      signature: TransactionSignature,
      confirmations?: number,
    ): Promise<RpcResponseAndContext<SignatureStatus | null>>;
    getSlot(commitment?: Commitment): Promise<number>;
    getSlotLeader(commitment?: Commitment): Promise<string>;
    getSignatureStatus(
      signature: TransactionSignature,
      config?: SignatureStatusConfig,
    ): Promise<RpcResponseAndContext<SignatureStatus | null>>;
    getSignatureStatuses(
      signatures: Array<TransactionSignature>,
      config?: SignatureStatusConfig,
    ): Promise<RpcResponseAndContext<Array<SignatureStatus | null>>>;
    getTransactionCount(commitment?: Commitment): Promise<number>;
    getTotalSupply(commitment?: Commitment): Promise<number>;
    getVersion(): Promise<Version>;
    getInflation(commitment?: Commitment): Promise<Inflation>;
    getEpochSchedule(): Promise<EpochSchedule>;
    getEpochInfo(commitment?: Commitment): Promise<EpochInfo>;
    getRecentBlockhashAndContext(
      commitment?: Commitment,
    ): Promise<RpcResponseAndContext<BlockhashAndFeeCalculator>>;
    getRecentBlockhash(
      commitment?: Commitment,
    ): Promise<BlockhashAndFeeCalculator>;
    requestAirdrop(
      to: PublicKey,
      amount: number,
      commitment?: Commitment,
    ): Promise<TransactionSignature>;
    sendTransaction(
      transaction: Transaction,
      signers: Array<Account>,
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
      commitment?: Commitment,
    ): number;
    removeAccountChangeListener(id: number): Promise<void>;
    onProgramAccountChange(
      programId: PublicKey,
      callback: ProgramAccountChangeCallback,
      commitment?: Commitment,
    ): number;
    removeProgramAccountChangeListener(id: number): Promise<void>;
    onSlotChange(callback: SlotChangeCallback): number;
    removeSlotChangeListener(id: number): Promise<void>;
    onSignature(
      signature: TransactionSignature,
      callback: SignatureResultCallback,
      commitment?: Commitment,
    ): number;
    removeSignatureListener(id: number): Promise<void>;
    onRootChange(callback: RootChangeCallback): number;
    removeRootChangeListener(id: number): Promise<void>;
    validatorExit(): Promise<boolean>;
    getMinimumBalanceForRentExemption(
      dataLength: number,
      commitment?: Commitment,
    ): Promise<number>;
    getNonce(
      nonceAccount: PublicKey,
      commitment?: Commitment,
    ): Promise<NonceAccount>;
    getNonceAndContext(
      nonceAccount: PublicKey,
      commitment?: Commitment,
    ): Promise<RpcResponseAndContext<NonceAccount>>;
  }

  // === src/nonce-account.js ===
  export class NonceAccount {
    authorizedPubkey: PublicKey;
    nonce: Blockhash;
    feeCalculator: FeeCalculator;
    static fromAccountData(
      buffer: Buffer | Uint8Array | Array<number>,
    ): NonceAccount;
  }

  export const NONCE_ACCOUNT_LENGTH: number;

  // === src/validator-info.js ===
  export const VALIDATOR_INFO_KEY: PublicKey;
  export type Info = {
    name: string;
    website?: string;
    details?: string;
    keybaseUsername?: string;
  };

  export class ValidatorInfo {
    key: PublicKey;
    info: Info;

    constructor(key: PublicKey, info: Info);
    static fromConfigData(
      buffer: Buffer | Uint8Array | Array<number>,
    ): ValidatorInfo | null;
  }

  // === src/sysvar.js ===
  export const SYSVAR_CLOCK_PUBKEY: PublicKey;
  export const SYSVAR_RENT_PUBKEY: PublicKey;
  export const SYSVAR_REWARDS_PUBKEY: PublicKey;
  export const SYSVAR_STAKE_HISTORY_PUBKEY: PublicKey;

  // === src/vote-account.js ===
  export const VOTE_PROGRAM_ID: PublicKey;
  export type Lockout = {
    slot: number;
    confirmationCount: number;
  };

  export type EpochCredits = {
    epoch: number;
    credits: number;
    prevCredits: number;
  };

  export class VoteAccount {
    votes: Array<Lockout>;
    nodePubkey: PublicKey;
    authorizedVoterPubkey: PublicKey;
    commission: number;
    rootSlot?: number;
    epoch: number;
    credits: number;
    lastEpochCredits: number;
    epochCredits: Array<EpochCredits>;
    static fromAccountData(
      buffer: Buffer | Uint8Array | Array<number>,
    ): VoteAccount;
  }

  // === src/instruction.js ===
  export type InstructionType = {
    index: number;
    layout: typeof BufferLayout;
  };

  export function encodeData(
    type: InstructionType,
    fields: Record<string, object>,
  ): Buffer;

  // === src/message.js ===
  export type MessageHeader = {
    numRequiredSignatures: number;
    numReadonlySignedAccounts: number;
    numReadonlyUnsignedAccounts: number;
  };

  export type CompiledInstruction = {
    programIdIndex: number;
    accounts: number[];
    data: string;
  };

  export type MessageArgs = {
    header: MessageHeader;
    accountKeys: PublicKey[];
    recentBlockhash: Blockhash;
    instructions: CompiledInstruction[];
  };

  export class Message {
    header: MessageHeader;
    accountKeys: PublicKey[];
    recentBlockhash: Blockhash;
    instructions: CompiledInstruction[];

    constructor(args: MessageArgs);
    isAccountWritable(index: number): boolean;
    serialize(): Buffer;
  }

  // === src/transaction.js ===
  export type TransactionSignature = string;

  export type AccountMeta = {
    pubkey: PublicKey;
    isSigner: boolean;
    isWritable: boolean;
  };

  export type TransactionInstructionCtorFields = {
    keys?: Array<AccountMeta>;
    programId?: PublicKey;
    data?: Buffer;
  };

  export class TransactionInstruction {
    keys: Array<AccountMeta>;
    programId: PublicKey;
    data: Buffer;

    constructor(opts?: TransactionInstructionCtorFields);
  }

  export type SignaturePubkeyPair = {
    signature?: Buffer;
    publicKey: PublicKey;
  };

  export type NonceInformation = {
    nonce: Blockhash;
    nonceInstruction: TransactionInstruction;
  };

  export type TransactionCtorFields = {
    recentBlockhash?: Blockhash;
    nonceInfo?: NonceInformation;
    signatures?: Array<SignaturePubkeyPair>;
  };

  export class Transaction {
    signatures: Array<SignaturePubkeyPair>;
    signature?: Buffer;
    instructions: Array<TransactionInstruction>;
    recentBlockhash?: Blockhash;
    nonceInfo?: NonceInformation;

    constructor(opts?: TransactionCtorFields);
    static from(buffer: Buffer | Uint8Array | Array<number>): Transaction;
    add(
      ...items: Array<
        Transaction | TransactionInstruction | TransactionInstructionCtorFields
      >
    ): Transaction;
    compileMessage(): Message;
    serializeMessage(): Buffer;
    sign(...signers: Array<Account>): void;
    signPartial(...partialSigners: Array<PublicKey | Account>): void;
    addSignature(pubkey: PublicKey, signature: Buffer): void;
    addSigner(signer: Account): void;
    verifySignatures(): boolean;
    serialize(): Buffer;
  }

  // === src/stake-program.js ===
  export type StakeAuthorizationType = {
    index: number;
  };

  export class Authorized {
    staker: PublicKey;
    withdrawer: PublicKey;
    constructor(staker: PublicKey, withdrawer: PublicKey);
  }

  export class Lockup {
    unixTimestamp: number;
    epoch: number;
    custodian: PublicKey;

    constructor(unixTimestamp: number, epoch: number, custodian: PublicKey);
  }

  export type CreateStakeAccountParams = {
    fromPubkey: PublicKey;
    stakePubkey: PublicKey;
    authorized: Authorized;
    lockup: Lockup;
    lamports: number;
  };

  export type CreateStakeAccountWithSeedParams = {
    fromPubkey: PublicKey;
    stakePubkey: PublicKey;
    basePubkey: PublicKey;
    seed: string;
    authorized: Authorized;
    lockup: Lockup;
    lamports: number;
  };

  export type InitializeStakeParams = {
    stakePubkey: PublicKey;
    authorized: Authorized;
    lockup: Lockup;
  };

  export type DelegateStakeParams = {
    stakePubkey: PublicKey;
    authorizedPubkey: PublicKey;
    votePubkey: PublicKey;
  };

  export type AuthorizeStakeParams = {
    stakePubkey: PublicKey;
    authorizedPubkey: PublicKey;
    newAuthorizedPubkey: PublicKey;
    stakeAuthorizationType: StakeAuthorizationType;
  };

  export type SplitStakeParams = {
    stakePubkey: PublicKey;
    authorizedPubkey: PublicKey;
    splitStakePubkey: PublicKey;
    lamports: number;
  };

  export type WithdrawStakeParams = {
    stakePubkey: PublicKey;
    authorizedPubkey: PublicKey;
    toPubkey: PublicKey;
    lamports: number;
  };

  export type DeactivateStakeParams = {
    stakePubkey: PublicKey;
    authorizedPubkey: PublicKey;
  };

  export class StakeProgram {
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

  export type StakeInstructionType =
    | 'Initialize'
    | 'Authorize'
    | 'Delegate'
    | 'Split'
    | 'Withdraw'
    | 'Deactivate';

  export const STAKE_INSTRUCTION_LAYOUTS: {
    [type in StakeInstructionType]: InstructionType;
  };

  export class StakeInstruction {
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
  export type CreateAccountParams = {
    fromPubkey: PublicKey;
    newAccountPubkey: PublicKey;
    lamports: number;
    space: number;
    programId: PublicKey;
  };

  export type CreateAccountWithSeedParams = {
    fromPubkey: PublicKey;
    newAccountPubkey: PublicKey;
    basePubkey: PublicKey;
    seed: string;
    lamports: number;
    space: number;
    programId: PublicKey;
  };

  export type AllocateParams = {
    accountPubkey: PublicKey;
    space: number;
  };

  export type AllocateWithSeedParams = {
    accountPubkey: PublicKey;
    basePubkey: PublicKey;
    seed: string;
    space: number;
    programId: PublicKey;
  };

  export type AssignParams = {
    accountPubkey: PublicKey;
    programId: PublicKey;
  };

  export type AssignWithSeedParams = {
    accountPubkey: PublicKey;
    basePubkey: PublicKey;
    seed: string;
    programId: PublicKey;
  };

  export type TransferParams = {
    fromPubkey: PublicKey;
    toPubkey: PublicKey;
    lamports: number;
  };

  export type CreateNonceAccountParams = {
    fromPubkey: PublicKey;
    noncePubkey: PublicKey;
    authorizedPubkey: PublicKey;
    lamports: number;
  };

  export type CreateNonceAccountWithSeedParams = {
    fromPubkey: PublicKey;
    noncePubkey: PublicKey;
    authorizedPubkey: PublicKey;
    lamports: number;
    basePubkey: PublicKey;
    seed: string;
  };

  export type InitializeNonceParams = {
    noncePubkey: PublicKey;
    authorizedPubkey: PublicKey;
  };

  export type AdvanceNonceParams = {
    noncePubkey: PublicKey;
    authorizedPubkey: PublicKey;
  };

  export type WithdrawNonceParams = {
    noncePubkey: PublicKey;
    authorizedPubkey: PublicKey;
    toPubkey: PublicKey;
    lamports: number;
  };

  export type AuthorizeNonceParams = {
    noncePubkey: PublicKey;
    authorizedPubkey: PublicKey;
    newAuthorizedPubkey: PublicKey;
  };

  export class SystemProgram {
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

  export type SystemInstructionType =
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

  export const SYSTEM_INSTRUCTION_LAYOUTS: {
    [type in SystemInstructionType]: InstructionType;
  };

  export class SystemInstruction {
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
  export class Loader {
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
  export class BpfLoader {
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
  export function sendAndConfirmTransaction(
    connection: Connection,
    transaction: Transaction,
    signers: Array<Account>,
    confirmations?: number,
  ): Promise<TransactionSignature>;

  // === src/util/send-and-confirm-raw-transaction.js ===
  export function sendAndConfirmRawTransaction(
    connection: Connection,
    wireTransaction: Buffer,
    confirmations?: number,
  ): Promise<TransactionSignature>;

  // === src/util/cluster.js ===
  export type Cluster = 'devnet' | 'testnet' | 'mainnet-beta';

  export function clusterApiUrl(cluster?: Cluster, tls?: boolean): string;

  // === src/index.js ===
  export const LAMPORTS_PER_SOL: number;
}
