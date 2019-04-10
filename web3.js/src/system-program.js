// @flow

import * as BufferLayout from 'buffer-layout';

import {Transaction} from './transaction';
import {PublicKey} from './publickey';
import * as Layout from './layout';

/**
 * Factory class for transactions to interact with the System program
 */
export class SystemProgram {
  /**
   * Public key that identifies the System program
   */
  static get programId(): PublicKey {
    return new PublicKey(
      '0x000000000000000000000000000000000000000000000000000000000000000',
    );
  }

  /**
   * Generate a Transaction that creates a new account
   */
  static createAccount(
    from: PublicKey,
    newAccount: PublicKey,
    lamports: number,
    space: number,
    programId: PublicKey,
  ): Transaction {
    const dataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('lamports'),
      BufferLayout.ns64('space'),
      Layout.publicKey('programId'),
    ]);

    const data = Buffer.alloc(dataLayout.span);
    dataLayout.encode(
      {
        instruction: 0, // Create Account instruction
        lamports,
        space,
        programId: programId.toBuffer(),
      },
      data,
    );

    return new Transaction().add({
      keys: [
        {pubkey: from, isSigner: true},
        {pubkey: newAccount, isSigner: false},
      ],
      programId: SystemProgram.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that moves lamports from one account to another
   */
  static move(from: PublicKey, to: PublicKey, amount: number): Transaction {
    const dataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('amount'),
    ]);

    const data = Buffer.alloc(dataLayout.span);
    dataLayout.encode(
      {
        instruction: 2, // Move instruction
        amount,
      },
      data,
    );

    return new Transaction().add({
      keys: [{pubkey: from, isSigner: true}, {pubkey: to, isSigner: false}],
      programId: SystemProgram.programId,
      data,
    });
  }

  /**
   * Generate a Transaction that assigns an account to a program
   */
  static assign(from: PublicKey, programId: PublicKey): Transaction {
    const dataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.publicKey('programId'),
    ]);

    const data = Buffer.alloc(dataLayout.span);
    dataLayout.encode(
      {
        instruction: 1, // Assign instruction
        programId: programId.toBuffer(),
      },
      data,
    );

    return new Transaction().add({
      keys: [{pubkey: from, isSigner: true}],
      programId: SystemProgram.programId,
      data,
    });
  }
}
