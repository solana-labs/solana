// @flow

import {Connection} from '../connection';
import {Transaction} from '../transaction';
import type {Account} from '../account';
import type {ConfirmOptions} from '../connection';
import type {TransactionSignature} from '../transaction';

/**
 * Sign, send and confirm a transaction.
 *
 * If `confirmations` count is not specified, wait for transaction to be finalized.
 *
 * @param {Connection} connection
 * @param {Transaction} transaction
 * @param {Array<Account>} signers
 * @param {ConfirmOptions} [options]
 * @returns {Promise<TransactionSignature>}
 */
export async function sendAndConfirmTransaction(
  connection: Connection,
  transaction: Transaction,
  signers: Array<Account>,
  options?: ConfirmOptions,
): Promise<TransactionSignature> {
  const start = Date.now();
  const signature = await connection.sendTransaction(
    transaction,
    signers,
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
        `Transaction ${signature} failed (${JSON.stringify(status)})`,
      );
    }
    return signature;
  }

  const duration = (Date.now() - start) / 1000;
  throw new Error(
    `Transaction was not confirmed in ${duration.toFixed(
      2,
    )} seconds (${JSON.stringify(status)})`,
  );
}
