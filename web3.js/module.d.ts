declare module '@solana/web3.js' {
  import * as BufferLayout from 'buffer-layout';

  // === src/publickey.js ===
  export class PublicKey {
    constructor(value: number | string | Buffer | Array<number>);
    static isPublicKey(o: object): boolean;
    equals(publickey: PublicKey): boolean;
    toBase58(): string;
    toBuffer(): Buffer;
    toString(): string;
  }

  // === src/blockhash.js ===
  export type Blockhash = string;

  // === src/account.js ===
  export class Account {
    constructor(secretKey?: Buffer);
    publicKey: PublicKey;
    secretKey: Buffer;
  }

  // === src/fee-calculator.js ===
  export type FeeCalculator = {
    burnPercent: number;
    lamportsPerSignature: number;
    maxLamportsPerSignature: number;
    minLamportsPerSignature: number;
    targetLamportsPerSignature: number;
    targetSignaturesPerSlot: number;
  };

  // === src/budget-program.js ===

  /* TODO */

  // === src/connection.js ===
  export type RpcResponseAndContext<T> = {
    context: {
      slot: number;
    };
    value: T;
  };

  export type Commitment = 'max' | 'recent';

  export type SignatureStatusResult = SignatureSuccess | TransactionError;

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
    gossip: string;
    tpu?: string;
    rpc?: string;
  };

  export type ConfirmedBlock = {
    blockhash: Blockhash;
    previousBlockhash: Blockhash;
    parentSlot: number;
    transactions: Array<{
      transaction: Transaction;
      meta: {
        fee: number;
        preBalances: Array<number>;
        postBalances: Array<number>;
        status?: SignatureStatusResult;
      };
    }>;
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

  export type AccountChangeCallback = (accountInfo: AccountInfo) => void;
  export type ProgramAccountChangeCallback = (
    keyedAccountInfo: KeyedAccountInfo,
  ) => void;
  export type SlotChangeCallback = (slotInfo: SlotInfo) => void;
  export type SignatureResultCallback = (
    signatureResult: SignatureStatusResult,
  ) => void;

  export type SignatureSuccess = {
    Ok: null;
  };
  export type TransactionError = {
    Err: object;
  };

  export type Inflation = {
    foundation: number;
    foundationTerm: number;
    initial: number;
    storage: number;
    taper: number;
    terminal: number;
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
    getAccountInfoAndContext(
      publicKey: PublicKey,
      commitment?: Commitment,
    ): Promise<RpcResponseAndContext<AccountInfo>>;
    getAccountInfo(
      publicKey: PublicKey,
      commitment?: Commitment,
    ): Promise<AccountInfo>;
    getProgramAccounts(
      programId: PublicKey,
      commitment?: Commitment,
    ): Promise<Array<PublicKeyAndAccount>>;
    getBalanceAndContext(
      publicKey: PublicKey,
      commitment?: Commitment,
    ): Promise<RpcResponseAndContext<number>>;
    getBalance(publicKey: PublicKey, commitment?: Commitment): Promise<number>;
    getClusterNodes(): Promise<Array<ContactInfo>>;
    getConfirmedBlock(): Promise<ConfirmedBlock>;
    getVoteAccounts(commitment?: Commitment): Promise<VoteAccountStatus>;
    confirmTransactionAndContext(
      signature: TransactionSignature,
      commitment?: Commitment,
    ): Promise<RpcResponseAndContext<boolean>>;
    confirmTransaction(
      signature: TransactionSignature,
      commitment?: Commitment,
    ): Promise<boolean>;
    getSlot(commitment?: Commitment): Promise<number>;
    getSlotLeader(commitment?: Commitment): Promise<string>;
    getSignatureStatus(
      signature: TransactionSignature,
      commitment?: Commitment,
    ): Promise<SignatureSuccess | TransactionError | null>;
    getTransactionCount(commitment?: Commitment): Promise<number>;
    getTotalSupply(commitment?: Commitment): Promise<number>;
    getVersion(): Promise<Version>;
    getInflation(commitment?: Commitment): Promise<Inflation>;
    getEpochSchedule(): Promise<EpochSchedule>;
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
      ...signers: Array<Account>
    ): Promise<TransactionSignature>;
    sendEncodedTransaction(
      encodedTransaction: string,
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
      commitment?: Commitment,
    ): Promise<number>;
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

  export class StakeProgram {
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
    static fromConfigData(buffer: Buffer): ValidatorInfo | null | undefined;
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
    static fromAccountData(buffer: Buffer): VoteAccount;
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

  // === src/transaction.js ===
  export type TransactionSignature = string;

  export type TransactionInstructionCtorFields = {
    keys?: Array<{pubkey: PublicKey; isSigner: boolean; isWritable: boolean}>;
    programId?: PublicKey;
    data?: Buffer;
  };

  export class TransactionInstruction {
    keys: Array<{
      pubkey: PublicKey;
      isSigner: boolean;
      isWritable: boolean;
    }>;
    programId: PublicKey;
    data: Buffer;

    constructor(opts?: TransactionInstructionCtorFields);
  }

  export type SignaturePubkeyPair = {
    signature?: Buffer;
    publicKey: PublicKey;
  };

  export type TransactionCtorFields = {
    recentBlockhash?: Blockhash;
    signatures?: Array<SignaturePubkeyPair>;
  };

  export class Transaction {
    signatures: Array<SignaturePubkeyPair>;
    signature?: Buffer;
    instructions: Array<TransactionInstruction>;
    recentBlockhash?: Blockhash;

    constructor(opts?: TransactionCtorFields);
    static from(buffer: Buffer): Transaction;
    add(
      ...items: Array<
        Transaction | TransactionInstruction | TransactionInstructionCtorFields
      >
    ): Transaction;
    sign(...signers: Array<Account>): void;
    signPartial(...partialSigners: Array<PublicKey | Account>): void;
    addSigner(signer: Account): void;
    serialize(): Buffer;
  }

  // === src/system-program.js ===
  export class SystemProgram {
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
    static createAccountWithSeed(
      from: PublicKey,
      newAccount: PublicKey,
      base: PublicKey,
      seed: string,
      lamports: number,
      space: number,
      programId: PublicKey,
    ): Transaction;
  }

  export class SystemInstruction extends TransactionInstruction {
    type: InstructionType;
    fromPublicKey: PublicKey | null;
    toPublicKey: PublicKey | null;
    amount: number | null;

    constructor(
      opts?: TransactionInstructionCtorFields,
      type?: InstructionType,
    );
    static from(instruction: TransactionInstruction): SystemInstruction;
  }

  // === src/loader.js ===
  export class Loader {
    static getMinNumSignatures(dataLength: number): number;
    static load(
      connection: Connection,
      payer: Account,
      program: Account,
      programId: PublicKey,
      data: Buffer | Array<number>,
    ): Promise<PublicKey>;
  }

  // === src/bpf-loader.js ===
  export class BpfLoader {
    static programId: PublicKey;
    static getMinNumSignatures(dataLength: number): number;
    static load(
      connection: Connection,
      payer: Account,
      elfBytes: Buffer | Array<number>,
    ): Promise<PublicKey>;
  }

  // === src/util/send-and-confirm-transaction.js ===
  export function sendAndConfirmTransaction(
    connection: Connection,
    transaction: Transaction,
    ...signers: Array<Account>
  ): Promise<TransactionSignature>;

  export function sendAndConfirmRecentTransaction(
    connection: Connection,
    transaction: Transaction,
    ...signers: Array<Account>
  ): Promise<TransactionSignature>;

  // === src/util/send-and-confirm-raw-transaction.js ===
  export function sendAndConfirmRawTransaction(
    connection: Connection,
    wireTransaction: Buffer,
    commitment?: Commitment,
  ): Promise<TransactionSignature>;

  // === src/util/testnet.js ===
  export function testnetChannelEndpoint(
    channel?: string,
    tls?: boolean,
  ): string;

  export const LAMPORTS_PER_SOL: number;
}
