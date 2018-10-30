// @flow

import * as BufferLayout from 'buffer-layout';

import {Account} from './account';
import {PublicKey} from './publickey';
import {SystemProgram} from './system-program';
import {Transaction} from './transaction';
import {sendAndConfirmTransaction} from './util/send-and-confirm-transaction';
import type {Connection} from './connection';

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
   * @param data Program data
   */
  async load(program: Account, data: Array<number>) {
    const userdataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.u32('offset'),
      BufferLayout.u32('bytesLength'),
      BufferLayout.u32('bytesLengthPadding'),
      BufferLayout.seq(
        BufferLayout.u8('byte'),
        BufferLayout.offset(BufferLayout.u32(), -8), 'bytes'),
    ]);

    const chunkSize = 256;
    let offset = 0;
    let array = data;
    const transactions = [];
    while (array.length > 0) {
      const bytes = array.slice(0, chunkSize);
      const userdata = Buffer.alloc(chunkSize + 16);
      userdataLayout.encode(
        {
          instruction: 0, // Load instruction
          offset,
          bytes,
        },
        userdata,
      );

      const transaction = new Transaction().add({
        keys: [program.publicKey],
        programId: this.programId,
        userdata,
      });
      transactions.push(sendAndConfirmTransaction(this.connection, program, transaction));

      offset += chunkSize;
      array = array.slice(chunkSize);
    }
    await Promise.all(transactions);
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

    const transaction = new Transaction();

    transaction.add({
      keys: [program.publicKey],
      programId: this.programId,
      userdata,
    });
    transaction.add(SystemProgram.spawn(program.publicKey));
    await sendAndConfirmTransaction(this.connection, program, transaction);
  }
}
