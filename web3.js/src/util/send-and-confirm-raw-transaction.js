// @flow

import {Connection} from '../connection';
import type {TransactionSignature} from '../transaction';
import type {ConfirmOptions} from '../connection';

/**
 * Send and confirm a raw transaction
 *
 * If `confirmations` count is not specified, wait for transaction to be finalized.
 *
 * @param {Connection} connection
 * @param {Buffer} rawTransaction
 * @param {ConfirmOptions} [options]
 * @returns {Promise<TransactionSignature>}
 */
export async function sendAndConfirmRawTransaction(
  connection: Connection,
  rawTransaction: Buffer,
  options?: ConfirmOptions,
): Promise<TransactionSignature> {
  const start = Date.now();
  const signature = await connection.sendRawTransaction(
    rawTransaction,
    options,
  );
  const status = (
    await connection.confirmTransaction(
      signature,
      options && options.confirmations,
    )
  ).value;

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
