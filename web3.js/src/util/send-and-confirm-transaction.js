// @flow

import {Connection} from '../connection';
import {Transaction} from '../transaction';
import {sleep} from './sleep';
import type {Account} from '../account';
import type {TransactionSignature} from '../transaction';

const NUM_SEND_RETRIES = 10;

/**
 * Sign, send and confirm a transaction.
 *
 * If `confirmations` count is not specified, wait for transaction to be finalized.
 */
export async function sendAndConfirmTransaction(
  connection: Connection,
  transaction: Transaction,
  signers: Array<Account>,
  confirmations: ?number,
): Promise<TransactionSignature> {
  const start = Date.now();
  let sendRetries = NUM_SEND_RETRIES;

  for (;;) {
    const signature = await connection.sendTransaction(transaction, signers);
    const status = (
      await connection.confirmTransaction(signature, confirmations)
    ).value;

    if (status) {
      if (status.err) {
        throw new Error(
          `Transaction ${signature} failed (${JSON.stringify(status)})`,
        );
      }
      return signature;
    }

    if (--sendRetries <= 0) break;

    // Retry in 0..100ms to try to avoid another AccountInUse collision
    await sleep(Math.random() * 100);
  }

  const duration = (Date.now() - start) / 1000;
  throw new Error(
    `Transaction was not confirmed in ${duration.toFixed(
      2,
    )} seconds (${JSON.stringify(status)})`,
  );
}
