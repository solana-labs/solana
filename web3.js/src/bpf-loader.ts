import type {Buffer} from 'buffer';

import {PublicKey} from './publickey';
import {Loader} from './loader';
import type {Connection} from './connection';
import type {Signer} from './keypair';

export const BPF_LOADER_PROGRAM_ID = new PublicKey(
  'BPFLoader2111111111111111111111111111111111',
);

/**
 * Factory class for transactions to interact with a program loader
 */
export class BpfLoader {
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
   * @param elf The entire ELF containing the BPF program
   * @param loaderProgramId The program id of the BPF loader to use
   * @return true if program was loaded successfully, false if program was already loaded
   */
  static load(
    connection: Connection,
    payer: Signer,
    program: Signer,
    elf: Buffer | Uint8Array | Array<number>,
    loaderProgramId: PublicKey,
  ): Promise<boolean> {
    return Loader.load(connection, payer, program, loaderProgramId, elf);
  }
}
