// @flow

import {Connection} from '../connection';
import type {TransactionSignature} from '../transaction';

/**
 * Send and confirm a raw transaction
 */
export async function sendAndConfirmRawTransaction(
  connection: Connection,
  rawTransaction: Buffer,
  confirmations: ?number,
): Promise<TransactionSignature> {
  const start = Date.now();
  const signature = await connection.sendRawTransaction(rawTransaction);
  const status = (await connection.confirmTransaction(signature, confirmations))
    .value;

  if (status) {
    if (status.err) {
      throw new Error(
        `Raw transaction ${signature} failed (${JSON.stringify(status)})`,
      );
    }
    return signature;
  }

  const duration = (Date.now() - start) / 1000;
  throw new Error(
    `Raw transaction '${signature}' was not confirmed in ${duration.toFixed(
      2,
    )} seconds`,
  );
}
