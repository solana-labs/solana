// @flow

import * as BufferLayout from 'buffer-layout';

import {Account} from './account';
import {PublicKey} from './publickey';
import {NUM_TICKS_PER_SECOND} from './timing';
import {Transaction} from './transaction';
import {sendAndConfirmTransaction} from './util/send-and-confirm-transaction';
import {sleep} from './util/sleep';
import type {Connection} from './connection';
import {SystemProgram} from './system-program';

/**
 * Program loader interface
 */
export class Loader {
  /**
   * Amount of program data placed in each load Transaction
   */
  static get chunkSize(): number {
    return 229; // Keep program chunks under PACKET_DATA_SIZE
  }

  /**
   * Loads a generic program
   *
   * @param connection The connection to use
   * @param payer System account that pays to load the program
   * @param program Account to load the program into
   * @param programId Public key that identifies the loader
   * @param data Program octets
   */
  static async load(
    connection: Connection,
    payer: Account,
    program: Account,
    programId: PublicKey,
    data: Array<number>,
  ): Promise<PublicKey> {
    {
      const transaction = SystemProgram.createAccount(
        payer.publicKey,
        program.publicKey,
        1,
        data.length,
        programId,
      );
      await sendAndConfirmTransaction(connection, transaction, payer);
    }

    const dataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
      BufferLayout.u32('offset'),
      BufferLayout.u32('bytesLength'),
      BufferLayout.u32('bytesLengthPadding'),
      BufferLayout.seq(
        BufferLayout.u8('byte'),
        BufferLayout.offset(BufferLayout.u32(), -8),
        'bytes',
      ),
    ]);

    const chunkSize = Loader.chunkSize;
    let offset = 0;
    let array = data;
    let transactions = [];
    while (array.length > 0) {
      const bytes = array.slice(0, chunkSize);
      const data = Buffer.alloc(chunkSize + 16);
      dataLayout.encode(
        {
          instruction: 0, // Load instruction
          offset,
          bytes,
        },
        data,
      );

      const transaction = new Transaction().add({
        keys: [{pubkey: program.publicKey, isSigner: true, isDebitable: true}],
        programId,
        data,
      });
      transactions.push(
        sendAndConfirmTransaction(connection, transaction, payer, program),
      );

      // Delay ~1 tick between write transactions in an attempt to reduce AccountInUse errors
      // since all the write transactions modify the same program account
      await sleep(1000 / NUM_TICKS_PER_SECOND);

      // Run up to 8 Loads in parallel to prevent too many parallel transactions from
      // getting rejected with AccountInUse.
      //
      // TODO: 8 was selected empirically and should probably be revisited
      if (transactions.length === 8) {
        await Promise.all(transactions);
        transactions = [];
      }

      offset += chunkSize;
      array = array.slice(chunkSize);
    }
    await Promise.all(transactions);

    // Finalize the account loaded with program data for execution
    {
      const dataLayout = BufferLayout.struct([BufferLayout.u32('instruction')]);

      const data = Buffer.alloc(dataLayout.span);
      dataLayout.encode(
        {
          instruction: 1, // Finalize instruction
        },
        data,
      );

      const transaction = new Transaction().add({
        keys: [{pubkey: program.publicKey, isSigner: true, isDebitable: true}],
        programId,
        data,
      });
      await sendAndConfirmTransaction(connection, transaction, payer, program);
    }
    return program.publicKey;
  }
}
