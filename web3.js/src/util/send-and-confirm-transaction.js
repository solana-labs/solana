// @flow

import {Connection, Transaction} from '..';

import {sleep} from './sleep';

import type {Account} from '..';

/**
 * Sign, send and confirm a transaction
 */
export async function sendAndConfirmTransaction(
  connection: Connection,
  from: Account,
  transaction: Transaction,
  runtimeErrorOk: boolean = false
): Promise<void> {
  const start = Date.now();
  const signature = await connection.sendTransaction(from, transaction);

  // Wait up to a couple seconds for a confirmation
  let i = 4;
  for (;;) {
    const status = await connection.getSignatureStatus(signature);
    switch (status) {
    case 'Confirmed':
      return;
    case 'ProgramRuntimeError':
      if (runtimeErrorOk) return;
      //fall through
    case 'GenericError':
    default:
      throw new Error(`Transaction ${signature} failed (${status})`);
    case 'SignatureNotFound':
      break;
    }

    await sleep(500);
    if (--i < 0) {
      const duration = (Date.now() - start) / 1000;
      throw new Error(`Transaction '${signature}' was not confirmed in ${duration.toFixed(2)} seconds (${status})`);
    }
  }
}

