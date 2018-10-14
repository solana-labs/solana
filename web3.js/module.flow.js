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

declare module '@solana/web3.js' {
  // === src/publickey.js ===
  declare export class PublicKey {
    constructor(number: string | Buffer | Array<number>): PublicKey;
    static isPublicKey(o: Object): boolean;
    equals(publickey: PublicKey): boolean;
    toBase58(): string;
    toBuffer(): Buffer;
  }

  // === src/account.js ===
  declare export class Account {
    constructor(secretKey: ?Buffer): Account;
    publicKey: PublicKey;
    secretKey: Buffer;
  }

  // === src/budget-program.js ===
  /* TODO */

  // === src/connection.js ===
  declare export type AccountInfo = {
    tokens: number,
    programId: PublicKey,
    userdata: Buffer,
  }

  declare export type SignatureStatus = 'Confirmed' | 'SignatureNotFound' | 'ProgramRuntimeError' | 'GenericFailure';

  declare export class Connection {
    constructor(endpoint: string): Connection;
    getBalance(publicKey: PublicKey): Promise<number>;
    getAccountInfo(publicKey: PublicKey): Promise<AccountInfo>;
    confirmTransaction(signature: TransactionSignature): Promise<boolean>;
    getSignatureStatus(signature: TransactionSignature): Promise<SignatureStatus>;
    getTransactionCount(): Promise<number>;
    getLastId(): Promise<TransactionId>;
    getFinality(): Promise<number>;
    requestAirdrop(to: PublicKey, amount: number): Promise<TransactionSignature>;
    sendTransaction(from: Account, transaction: Transaction): Promise<TransactionSignature>;
  }

  // === src/system-program.js ===
  declare export class SystemProgram {
    static programId: PublicKey;

    static createAccount(
      from: PublicKey,
      newAccount: PublicKey,
      tokens: number,
      space: number,
      programId: PublicKey
    ): Transaction;
    static move(from: PublicKey, to: PublicKey, amount: number): Transaction;
    static assign(from: PublicKey, programId: PublicKey): Transaction;
    static load(from: PublicKey, programId: PublicKey, name: string): Transaction;
  }

  // === src/transaction.js ===
  declare export type TransactionSignature = string;
  declare export type TransactionId = string;

  declare type TransactionCtorFields = {|
    signature?: Buffer;
    keys?: Array<PublicKey>;
    programId?: PublicKey;
    fee?: number;
    userdata?: Buffer;
  |};


  declare export class Transaction {
    signature: ?Buffer;
    keys: Array<PublicKey>;
    programId: ?PublicKey;
    lastId: ?TransactionId;
    fee: number;
    userdata: Buffer;

    constructor(opts?: TransactionCtorFields): Transaction;
    sign(from: Account): void;
    serialize(): Buffer;
  }

  // === src/token-program.js ===
  /* TODO */

}
