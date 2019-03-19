// @flow

import {Connection} from '../connection';
import {sleep} from './sleep';
import type {TransactionSignature} from '../transaction';
import {DEFAULT_TICKS_PER_SLOT, NUM_TICKS_PER_SECOND} from '../timing';

/**
 * Sign, send and confirm a raw transaction
 */
export async function sendAndConfirmRawTransaction(
  connection: Connection,
  rawTransaction: Buffer,
): Promise<TransactionSignature> {
  const start = Date.now();
  let signature = await connection.sendRawTransaction(rawTransaction);

  // Wait up to a couple slots for a confirmation
  let status = '';
  let statusRetries = 6;
  for (;;) {
    status = await connection.getSignatureStatus(signature);
    if (status !== 'SignatureNotFound') {
      break;
    }

    // Sleep for approximately half a slot
    await sleep((500 * DEFAULT_TICKS_PER_SLOT) / NUM_TICKS_PER_SECOND);

    if (--statusRetries <= 0) {
      const duration = (Date.now() - start) / 1000;
      throw new Error(
        `Raw Transaction '${signature}' was not confirmed in ${duration.toFixed(
          2,
        )} seconds (${status})`,
      );
    }
  }

  if (status === 'Confirmed') {
    return signature;
  }

  throw new Error(`Raw transaction ${signature} failed (${status})`);
}
