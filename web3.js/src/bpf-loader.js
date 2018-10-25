// @flow

import elfy from 'elfy';

import {Account, PublicKey, Loader, SystemProgram} from '.';
import {sendAndConfirmTransaction} from './util/send-and-confirm-transaction';
import type {Connection} from '.';

/**
 * Factory class for transactions to interact with a program loader
 */
export class BpfLoader {
  /**
   * Public key that identifies the BpfLoader
   */
  static get programId(): PublicKey {
    return new PublicKey('0x0606060606060606060606060606060606060606060606060606060606060606');
  }

  /**
   * Load a BPF program
   *
   * @param connection The connection to use
   * @param owner User account to load the program into
   * @param elfBytes the entire ELF containing the BPF program in its .text.entrypoint section
   */
  static async load(
    connection: Connection,
    owner: Account,
    elfBytes: Array<number>,
  ): Promise<PublicKey> {
    const programAccount = new Account();

    const elf = elfy.parse(elfBytes);
    const section = elf.body.sections.find(section => section.name === '.text.entrypoint');

    const transaction = SystemProgram.createAccount(
      owner.publicKey,
      programAccount.publicKey,
      1,
      section.data.length,
      BpfLoader.programId,
    );
    await sendAndConfirmTransaction(connection, owner, transaction);

    const loader = new Loader(connection, BpfLoader.programId);
    await loader.load(programAccount, section.data);
    await loader.finalize(programAccount);

    return programAccount.publicKey;
  }
}
