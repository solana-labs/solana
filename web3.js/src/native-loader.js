// @flow

import {Account} from './account';
import {PublicKey} from './publickey';
import {Loader} from './loader';
import type {Connection} from './connection';

/**
 * Factory class for transactions to interact with a program loader
 */
export class NativeLoader {
  /**
   * Public key that identifies the NativeLoader
   */
  static get programId(): PublicKey {
    return new PublicKey('NativeLoader1111111111111111111111111111111');
  }

  /**
   * Loads a native program
   *
   * @param connection The connection to use
   * @param payer System account that pays to load the program
   * @param programName Name of the native program
   */
  static load(
    connection: Connection,
    payer: Account,
    programName: string,
  ): Promise<PublicKey> {
    const bytes = [...Buffer.from(programName)];
    const program = new Account();
    return Loader.load(
      connection,
      payer,
      program,
      NativeLoader.programId,
      bytes,
    );
  }
}
