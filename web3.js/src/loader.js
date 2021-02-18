// @flow

import {Buffer} from 'buffer';
import * as BufferLayout from 'buffer-layout';

import {Account} from './account';
import {PublicKey} from './publickey';
import {Transaction, PACKET_DATA_SIZE} from './transaction';
import {SYSVAR_RENT_PUBKEY} from './sysvar';
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
    // Keep program chunks under PACKET_DATA_SIZE, leaving enough room for the
    // rest of the Transaction fields
    //
    // TODO: replace 300 with a proper constant for the size of the other
    // Transaction fields
    return PACKET_DATA_SIZE - 300;
  }

  /**
   * Minimum number of signatures required to load a program not including
   * retries
   *
   * Can be used to calculate transaction fees
   */
  static getMinNumSignatures(dataLength: number): number {
    return (
      2 * // Every transaction requires two signatures (payer + program)
      (Math.ceil(dataLength / Loader.chunkSize) +
        1 + // Add one for Create transaction
        1) // Add one for Finalize transaction
    );
  }

  /**
   * Loads a generic program
   *
   * @param connection The connection to use
   * @param payer System account that pays to load the program
   * @param program Account to load the program into
   * @param programId Public key that identifies the loader
   * @param data Program octets
   * @return true if program was loaded successfully, false if program was already loaded
   */
  static async load(
    connection: Connection,
    payer: Account,
    program: Account,
    programId: PublicKey,
    data: Buffer | Uint8Array | Array<number>,
  ): Promise<boolean> {
    {
      const balanceNeeded = await connection.getMinimumBalanceForRentExemption(
        data.length,
      );

      // Fetch program account info to check if it has already been created
      const programInfo = await connection.getAccountInfo(
        program.publicKey,
        'confirmed',
      );

      let transaction: Transaction | null = null;
      if (programInfo !== null) {
        if (programInfo.executable) {
          console.error('Program load failed, account is already executable');
          return false;
        }

        if (programInfo.data.length !== data.length) {
          transaction = transaction || new Transaction();
          transaction.add(
            SystemProgram.allocate({
              accountPubkey: program.publicKey,
              space: data.length,
            }),
          );
        }

        if (!programInfo.owner.equals(programId)) {
          transaction = transaction || new Transaction();
          transaction.add(
            SystemProgram.assign({
              accountPubkey: program.publicKey,
              programId,
            }),
          );
        }

        if (programInfo.lamports < balanceNeeded) {
          transaction = transaction || new Transaction();
          transaction.add(
            SystemProgram.transfer({
              fromPubkey: payer.publicKey,
              toPubkey: program.publicKey,
              lamports: balanceNeeded - programInfo.lamports,
            }),
          );
        }
      } else {
        transaction = new Transaction().add(
          SystemProgram.createAccount({
            fromPubkey: payer.publicKey,
            newAccountPubkey: program.publicKey,
            lamports: balanceNeeded > 0 ? balanceNeeded : 1,
            space: data.length,
            programId,
          }),
        );
      }

      // If the account is already created correctly, skip this step
      // and proceed directly to loading instructions
      if (transaction !== null) {
        await sendAndConfirmTransaction(
          connection,
          transaction,
          [payer, program],
          {
            commitment: 'confirmed',
          },
        );
      }
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
        keys: [{pubkey: program.publicKey, isSigner: true, isWritable: true}],
        programId,
        data,
      });
      transactions.push(
        sendAndConfirmTransaction(connection, transaction, [payer, program], {
          commitment: 'confirmed',
        }),
      );

      // Delay between sends in an attempt to reduce rate limit errors
      if (connection._rpcEndpoint.includes('solana.com')) {
        const REQUESTS_PER_SECOND = 4;
        await sleep(1000 / REQUESTS_PER_SECOND);
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
        keys: [
          {pubkey: program.publicKey, isSigner: true, isWritable: true},
          {pubkey: SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false},
        ],
        programId,
        data,
      });
      await sendAndConfirmTransaction(
        connection,
        transaction,
        [payer, program],
        {
          commitment: 'confirmed',
        },
      );
    }

    // success
    return true;
  }
}
