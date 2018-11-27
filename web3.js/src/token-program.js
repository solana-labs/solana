/**
 * @flow
 */

import assert from 'assert';
import BN from 'bn.js';
import * as BufferLayout from 'buffer-layout';

import * as Layout from './layout';
import {Account} from './account';
import {PublicKey} from './publickey';
import {SystemProgram} from './system-program';
import {Transaction, TransactionInstruction} from './transaction';
import type {TransactionSignature} from './transaction';
import {sendAndConfirmTransaction} from './util/send-and-confirm-transaction';
import type {Connection} from './connection';

/**
 * Some amount of tokens
 */
export class TokenAmount extends BN {
  /**
   * Convert to Buffer representation
   */
  toBuffer(): Buffer {
    const a = super.toArray().reverse();
    const b = Buffer.from(a);
    if (b.length === 8) {
      return b;
    }
    assert(b.length < 8, 'TokenAmount too large');

    const zeroPad = Buffer.alloc(8);
    b.copy(zeroPad);
    return zeroPad;
  }

  /**
   * Construct a TokenAmount from Buffer representation
   */
  static fromBuffer(buffer: Buffer): TokenAmount {
    assert(buffer.length === 8, `Invalid buffer length: ${buffer.length}`);
    return new BN(
      [...buffer]
        .reverse()
        .map(i => `00${i.toString(16)}`.slice(-2))
        .join(''),
      16,
    );
  }
}

/**
 * Information about a token
 */
type TokenInfo = {|
  /**
   * Total supply of tokens
   */
  supply: TokenAmount,

  /**
   * Number of base 10 digits to the right of the decimal place
   */
  decimals: number,

  /**
   * Descriptive name of this token
   */
  name: string,

  /**
   * Symbol for this token
   */
  symbol: string,
|};

/**
 * @private
 */
const TokenInfoLayout = BufferLayout.struct([
  Layout.uint64('supply'),
  BufferLayout.u8('decimals'),
  Layout.rustString('name'),
  Layout.rustString('symbol'),
]);

/**
 * Information about a token account
 */
type TokenAccountInfo = {|
  /**
   * The kind of token this account holds
   */
  token: PublicKey,

  /**
   * Owner of this account
   */
  owner: PublicKey,

  /**
   * Amount of tokens this account holds
   */
  amount: TokenAmount,

  /**
   * The source account for the tokens.
   *
   * If `source` is null, the source is this account.
   * If `source` is not null, the `amount` of tokens in this account represent
   * an allowance of tokens that may be transferred from the source account
   */
  source: null | PublicKey,

  /**
   * Original amount of tokens this delegate account was authorized to spend
   * If `source` is null, originalAmount is zero
   */
  originalAmount: TokenAmount,
|};

/**
 * @private
 */
const TokenAccountInfoLayout = BufferLayout.struct([
  Layout.publicKey('token'),
  Layout.publicKey('owner'),
  Layout.uint64('amount'),
  BufferLayout.u8('sourceOption'),
  Layout.publicKey('source'),
  Layout.uint64('originalAmount'),
]);

type TokenAndPublicKey = [Token, PublicKey]; // This type exists to workaround an esdoc parse error

/**
 * The built-in token program
 */
export const SYSTEM_TOKEN_PROGRAM_ID = new PublicKey(
  '0x8300000000000000000000000000000000000000000000000000000000000000',
);

/**
 * An ERC20-like Token
 */
export class Token {
  /**
   * @private
   */
  connection: Connection;

  /**
   * The public key identifying this token
   */
  token: PublicKey;

  /**
   * Program Identifier for the Token program
   */
  programId: PublicKey;

  /**
   * Create a Token object attached to the specific token
   *
   * @param connection The connection to use
   * @param token Public key of the token
   * @param programId Optional token programId, uses the system programId by default
   */
  constructor(
    connection: Connection,
    token: PublicKey,
    programId: PublicKey = SYSTEM_TOKEN_PROGRAM_ID,
  ) {
    Object.assign(this, {connection, token, programId});
  }

  /**
   * Create a new Token
   *
   * @param connection The connection to use
   * @param owner User account that will own the returned Token Account
   * @param supply Total supply of the new token
   * @param name Descriptive name of this token
   * @param symbol Symbol for this token
   * @param decimals Location of the decimal place
   * @param programId Optional token programId, uses the system programId by default
   * @return Token object for the newly minted token, Public key of the Token Account holding the total supply of new tokens
   */
  static async createNewToken(
    connection: Connection,
    owner: Account,
    supply: TokenAmount,
    name: string,
    symbol: string,
    decimals: number,
    programId: PublicKey = SYSTEM_TOKEN_PROGRAM_ID,
  ): Promise<TokenAndPublicKey> {
    const tokenAccount = new Account();
    const token = new Token(connection, tokenAccount.publicKey, programId);
    const initialAccountPublicKey = await token.newAccount(owner, null);

    let transaction;

    const userdataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.uint64('supply'),
      BufferLayout.u8('decimals'),
      Layout.rustString('name'),
      Layout.rustString('symbol'),
    ]);

    let userdata = Buffer.alloc(1024);
    {
      const encodeLength = userdataLayout.encode(
        {
          instruction: 0, // NewToken instruction
          supply: supply.toBuffer(),
          decimals,
          name,
          symbol,
        },
        userdata,
      );
      userdata = userdata.slice(0, encodeLength);
    }

    // Allocate memory for the tokenAccount account
    transaction = SystemProgram.createAccount(
      owner.publicKey,
      tokenAccount.publicKey,
      1,
      1 + userdata.length,
      programId,
    );
    await sendAndConfirmTransaction(connection, transaction, owner);

    transaction = new Transaction().add({
      keys: [tokenAccount.publicKey, initialAccountPublicKey],
      programId,
      userdata,
    });
    transaction.fee = 0; // TODO: Batch with the `SystemProgram.createAccount` and remove this line
    await sendAndConfirmTransaction(connection, transaction, tokenAccount);

    return [token, initialAccountPublicKey];
  }

  /**
   * Create a new and empty token account.
   *
   * This account may then be used as a `transfer()` or `approve()` destination
   *
   * @param owner User account that will own the new token account
   * @param source If not null, create a delegate account that when authorized
   *               may transfer tokens from this `source` account
   * @return Public key of the new empty token account
   */
  async newAccount(
    owner: Account,
    source: null | PublicKey = null,
  ): Promise<PublicKey> {
    const tokenAccount = new Account();
    let transaction;

    const userdataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
    ]);

    const userdata = Buffer.alloc(userdataLayout.span);
    userdataLayout.encode(
      {
        instruction: 1, // NewTokenAccount instruction
      },
      userdata,
    );

    // Allocate memory for the token
    transaction = SystemProgram.createAccount(
      owner.publicKey,
      tokenAccount.publicKey,
      1,
      1 + TokenAccountInfoLayout.span,
      this.programId,
    );
    await sendAndConfirmTransaction(this.connection, transaction, owner);

    // Initialize the token account
    const keys = [tokenAccount.publicKey, owner.publicKey, this.token];
    if (source) {
      keys.push(source);
    }
    transaction = new Transaction().add({
      keys,
      programId: this.programId,
      userdata,
    });
    transaction.fee = 0; // TODO: Batch with the `SystemProgram.createAccount` and remove this line
    await sendAndConfirmTransaction(this.connection, transaction, tokenAccount);

    return tokenAccount.publicKey;
  }

  /**
   * Retrieve token information
   */
  async tokenInfo(): Promise<TokenInfo> {
    const accountInfo = await this.connection.getAccountInfo(this.token);
    if (!accountInfo.owner.equals(this.programId)) {
      throw new Error(
        `Invalid token owner: ${JSON.stringify(accountInfo.owner)}`,
      );
    }

    const userdata = Buffer.from(accountInfo.userdata);

    if (userdata.readUInt8(0) !== 1) {
      throw new Error(`Invalid token userdata`);
    }
    const tokenInfo = TokenInfoLayout.decode(userdata, 1);
    tokenInfo.supply = TokenAmount.fromBuffer(tokenInfo.supply);
    return tokenInfo;
  }

  /**
   * Retrieve account information
   *
   * @param account Public key of the token account
   */
  async accountInfo(account: PublicKey): Promise<TokenAccountInfo> {
    const accountInfo = await this.connection.getAccountInfo(account);
    if (!accountInfo.owner.equals(this.programId)) {
      throw new Error(`Invalid token account owner`);
    }

    const userdata = Buffer.from(accountInfo.userdata);
    if (userdata.readUInt8(0) !== 2) {
      throw new Error(`Invalid token account userdata`);
    }
    const tokenAccountInfo = TokenAccountInfoLayout.decode(userdata, 1);

    tokenAccountInfo.token = new PublicKey(tokenAccountInfo.token);
    tokenAccountInfo.owner = new PublicKey(tokenAccountInfo.owner);
    tokenAccountInfo.amount = TokenAmount.fromBuffer(tokenAccountInfo.amount);
    if (tokenAccountInfo.sourceOption === 0) {
      tokenAccountInfo.source = null;
      tokenAccountInfo.originalAmount = new TokenAmount();
    } else {
      tokenAccountInfo.source = new PublicKey(tokenAccountInfo.source);
      tokenAccountInfo.originalAmount = TokenAmount.fromBuffer(
        tokenAccountInfo.originalAmount,
      );
    }

    if (!tokenAccountInfo.token.equals(this.token)) {
      throw new Error(
        `Invalid token account token: ${JSON.stringify(
          tokenAccountInfo.token,
        )} !== ${JSON.stringify(this.token)}`,
      );
    }
    return tokenAccountInfo;
  }

  /**
   * Transfer tokens to another account
   *
   * @param owner Owner of the source token account
   * @param source Source token account
   * @param destination Destination token account
   * @param amount Number of tokens to transfer
   */
  async transfer(
    owner: Account,
    source: PublicKey,
    destination: PublicKey,
    amount: number | TokenAmount,
  ): Promise<?TransactionSignature> {
    return await sendAndConfirmTransaction(
      this.connection,
      new Transaction().add(
        await this.transferInstruction(
          owner.publicKey,
          source,
          destination,
          amount,
        ),
      ),
      owner,
    );
  }

  /**
   * Grant a third-party permission to transfer up the specified number of tokens from an account
   *
   * @param owner Owner of the source token account
   * @param account Public key of the token account
   * @param delegate Token account authorized to perform a transfer tokens from the source account
   * @param amount Maximum number of tokens the delegate may transfer
   */
  async approve(
    owner: Account,
    account: PublicKey,
    delegate: PublicKey,
    amount: number | TokenAmount,
  ): Promise<void> {
    await sendAndConfirmTransaction(
      this.connection,
      new Transaction().add(
        this.approveInstruction(owner.publicKey, account, delegate, amount),
      ),
      owner,
    );
  }

  /**
   * Remove approval for the transfer of any remaining tokens
   *
   * @param owner Owner of the source token account
   * @param account Public key of the token account
   * @param delegate Token account to revoke authorization from
   */
  revoke(
    owner: Account,
    account: PublicKey,
    delegate: PublicKey,
  ): Promise<void> {
    return this.approve(owner, account, delegate, 0);
  }

  /**
   * Assign a new owner to the account
   *
   * @param owner Owner of the token account
   * @param account Public key of the token account
   * @param newOwner New owner of the token account
   */
  async setOwner(
    owner: Account,
    account: PublicKey,
    newOwner: PublicKey,
  ): Promise<void> {
    await sendAndConfirmTransaction(
      this.connection,
      new Transaction().add(
        this.setOwnerInstruction(owner.publicKey, account, newOwner),
      ),
      owner,
    );
  }

  /**
   * Construct a Transfer instruction
   *
   * @param owner Owner of the source token account
   * @param source Source token account
   * @param destination Destination token account
   * @param amount Number of tokens to transfer
   */
  async transferInstruction(
    owner: PublicKey,
    source: PublicKey,
    destination: PublicKey,
    amount: number | TokenAmount,
  ): Promise<TransactionInstruction> {
    const accountInfo = await this.accountInfo(source);
    if (!owner.equals(accountInfo.owner)) {
      throw new Error('Account owner mismatch');
    }

    const userdataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.uint64('amount'),
    ]);

    const userdata = Buffer.alloc(userdataLayout.span);
    userdataLayout.encode(
      {
        instruction: 2, // Transfer instruction
        amount: new TokenAmount(amount).toBuffer(),
      },
      userdata,
    );

    const keys = [owner, source, destination];
    if (accountInfo.source) {
      keys.push(accountInfo.source);
    }
    return new TransactionInstruction({
      keys,
      programId: this.programId,
      userdata,
    });
  }

  /**
   * Construct an Approve instruction
   *
   * @param owner Owner of the source token account
   * @param account Public key of the token account
   * @param delegate Token account authorized to perform a transfer tokens from the source account
   * @param amount Maximum number of tokens the delegate may transfer
   */
  approveInstruction(
    owner: PublicKey,
    account: PublicKey,
    delegate: PublicKey,
    amount: number | TokenAmount,
  ): TransactionInstruction {
    const userdataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.uint64('amount'),
    ]);

    const userdata = Buffer.alloc(userdataLayout.span);
    userdataLayout.encode(
      {
        instruction: 3, // Approve instruction
        amount: new TokenAmount(amount).toBuffer(),
      },
      userdata,
    );

    return new TransactionInstruction({
      keys: [owner, account, delegate],
      programId: this.programId,
      userdata,
    });
  }

  /**
   * Construct an Revoke instruction
   *
   * @param owner Owner of the source token account
   * @param account Public key of the token account
   * @param delegate Token account authorized to perform a transfer tokens from the source account
   */
  revokeInstruction(
    owner: PublicKey,
    account: PublicKey,
    delegate: PublicKey,
  ): TransactionInstruction {
    return this.approveInstruction(owner, account, delegate, 0);
  }

  /**
   * Construct a SetOwner instruction
   *
   * @param owner Owner of the token account
   * @param account Public key of the token account
   * @param newOwner New owner of the token account
   */
  setOwnerInstruction(
    owner: PublicKey,
    account: PublicKey,
    newOwner: PublicKey,
  ): TransactionInstruction {
    const userdataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
    ]);

    const userdata = Buffer.alloc(userdataLayout.span);
    userdataLayout.encode(
      {
        instruction: 4, // SetOwner instruction
      },
      userdata,
    );

    return new TransactionInstruction({
      keys: [owner, account, newOwner],
      programId: this.programId,
      userdata,
    });
  }
}
