// @flow

import * as BufferLayout from 'buffer-layout';

import {PublicKey, Transaction} from '.';
import {sendAndConfirmTransaction} from './util/send-and-confirm-transaction';
import type {Account, Connection} from '.';

/**
 * Program loader interface
 */
export class Loader {
  /**
   * @private
   */
  connection: Connection;

  /**
   * @private
   */
  programId: PublicKey;

  /**
   * @param connection The connection to use
   * @param programId Public key that identifies the loader
   */
  constructor(connection: Connection, programId: PublicKey) {
    Object.assign(this, {connection, programId});
  }

  /**
   * Load program data
   *
   * @param program Account to load the program info
   * @param offset Account userdata offset to write `bytes` into
   * @param bytes Program data
   */
  async load(program: Account, offset: number, bytes: Array<number>) {
    const userdataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.u32('offset'),
      BufferLayout.u32('bytesLength'),
      BufferLayout.u32('bytesLengthPadding'),
      BufferLayout.seq(
        BufferLayout.u8('byte'),
        BufferLayout.offset(BufferLayout.u32(), -8),
        'bytes'
      ),
    ]);

    let userdata = Buffer.alloc(bytes.length + 16);
    userdataLayout.encode(
      {
        instruction: 0, // Load instruction
        offset,
        bytes,
      },
      userdata,
    );

    const transaction = new Transaction({
      fee: 0,
      keys: [program.publicKey],
      programId: this.programId,
      userdata,
    });
    await sendAndConfirmTransaction(this.connection, program, transaction);
  }

  /**
   * Finalize an account loaded with program data for execution
   *
   * @param program `load()`ed Account
   */
  async finalize(program: Account) {
    const userdataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
    ]);

    const userdata = Buffer.alloc(userdataLayout.span);
    userdataLayout.encode(
      {
        instruction: 1, // Finalize instruction
      },
      userdata,
    );

    const transaction = new Transaction({
      fee: 0,
      keys: [program.publicKey],
      programId: this.programId,
      userdata,
    });
    await sendAndConfirmTransaction(this.connection, program, transaction);
  }
}
