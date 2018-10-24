// @flow

import fs from 'mz/fs';
import elfy from 'elfy';

import {Account, PublicKey, Loader, SystemProgram} from '.';
import {sendAndConfirmTransaction} from './util/send-and-confirm-transaction';
import type {Connection} from '.';

/**
 * Factory class for transactions to interact with a program loader
 */
export class BpfLoader {
  /**
   * Public key that identifies the NativeLoader
   */
  static get programId(): PublicKey {
    return new PublicKey('0x0606060606060606060606060606060606060606060606060606060606060606');
  }

  /**
   * Loads a BPF program
   *
   * @param connection The connection to use
   * @param owner User account to load the program with
   * @param programName Name of the BPF program
   */
  static async load(
    connection: Connection,
    owner: Account,
    programName: string,
  ): Promise<PublicKey> {
    const programAccount = new Account();

    const data = await fs.readFile(programName);
    const elf = elfy.parse(data);
    const section = elf.body.sections.find(section => section.name === '.text.entrypoint');

    // Allocate memory for the program account
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
