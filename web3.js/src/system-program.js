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
    const userdataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('lamports'),
      BufferLayout.ns64('space'),
      Layout.publicKey('programId'),
    ]);

    const userdata = Buffer.alloc(userdataLayout.span);
    userdataLayout.encode(
      {
        instruction: 0, // Create Account instruction
        lamports,
        space,
        programId: programId.toBuffer(),
      },
      userdata,
    );

    return new Transaction().add({
      keys: [from, newAccount],
      programId: SystemProgram.programId,
      userdata,
    });
  }

  /**
   * Generate a Transaction that moves lamports from one account to another
   */
  static move(from: PublicKey, to: PublicKey, amount: number): Transaction {
    const userdataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.ns64('amount'),
    ]);

    const userdata = Buffer.alloc(userdataLayout.span);
    userdataLayout.encode(
      {
        instruction: 2, // Move instruction
        amount,
      },
      userdata,
    );

    return new Transaction().add({
      keys: [from, to],
      programId: SystemProgram.programId,
      userdata,
    });
  }

  /**
   * Generate a Transaction that assigns an account to a program
   */
  static assign(from: PublicKey, programId: PublicKey): Transaction {
    const userdataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
      Layout.publicKey('programId'),
    ]);

    const userdata = Buffer.alloc(userdataLayout.span);
    userdataLayout.encode(
      {
        instruction: 1, // Assign instruction
        programId: programId.toBuffer(),
      },
      userdata,
    );

    return new Transaction().add({
      keys: [from],
      programId: SystemProgram.programId,
      userdata,
    });
  }
}
