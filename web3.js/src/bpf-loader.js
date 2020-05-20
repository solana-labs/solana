// @flow

import {Account} from './account';
import {PublicKey} from './publickey';
import {Loader} from './loader';
import type {Connection} from './connection';

/**
 * Factory class for transactions to interact with a program loader
 */
export class BpfLoader {
  /**
   * Public key that identifies the BpfLoader
   */
  static get programId(): PublicKey {
    return new PublicKey('BPFLoader1111111111111111111111111111111111');
  }

  /**
   * Minimum number of signatures required to load a program not including
   * retries
   *
   * Can be used to calculate transaction fees
   */
  static getMinNumSignatures(dataLength: number): number {
    return Loader.getMinNumSignatures(dataLength);
  }

  /**
   * Load a BPF program
   *
   * @param connection The connection to use
   * @param payer Account that will pay program loading fees
   * @param program Account to load the program into
   * @param elfBytes The entire ELF containing the BPF program
   */
  static load(
    connection: Connection,
    payer: Account,
    program: Account,
    elf: Buffer | Uint8Array | Array<number>,
  ): Promise<void> {
    return Loader.load(connection, payer, program, BpfLoader.programId, elf);
  }
}
