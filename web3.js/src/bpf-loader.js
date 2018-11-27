// @flow

import {Account} from './account';
import {PublicKey} from './publickey';
import {Loader} from './loader';
import {SystemProgram} from './system-program';
import {sendAndConfirmTransaction} from './util/send-and-confirm-transaction';
import type {Connection} from './connection';

/**
 * Factory class for transactions to interact with a program loader
 */
export class BpfLoader {
  /**
   * Public key that identifies the BpfLoader
   */
  static get programId(): PublicKey {
    return new PublicKey(
      '0x8000000000000000000000000000000000000000000000000000000000000000',
    );
  }

  /**
   * Load a BPF program
   *
   * @param connection The connection to use
   * @param owner User account to load the program into
   * @param elfBytes The entire ELF containing the BPF program
   */
  static async load(
    connection: Connection,
    owner: Account,
    elf: Array<number>,
  ): Promise<PublicKey> {
    const programAccount = new Account();

    const transaction = SystemProgram.createAccount(
      owner.publicKey,
      programAccount.publicKey,
      1 + Math.ceil(elf.length / Loader.chunkSize) + 1,
      elf.length,
      BpfLoader.programId,
    );
    await sendAndConfirmTransaction(connection, transaction, owner);

    const loader = new Loader(connection, BpfLoader.programId);
    await loader.load(programAccount, elf);
    await loader.finalize(programAccount);

    return programAccount.publicKey;
  }
}
