// @flow

import invariant from 'assert';

import {Connection} from '../connection';
import {Transaction} from '../transaction';
import {sleep} from './sleep';
import type {Account} from '../account';
import type {TransactionSignature} from '../transaction';

/**
 * Sign, send and confirm a transaction
 */
export async function sendAndConfirmTransaction(
  connection: Connection,
  transaction: Transaction,
  ...signers: Array<Account>
): Promise<TransactionSignature> {
  let sendRetries = 10;
  let signature;
  for (;;) {
    const start = Date.now();
    signature = await connection.sendTransaction(transaction, ...signers);

    // Wait up to a couple seconds for a confirmation
    let status = 'SignatureNotFound';
    let statusRetries = 4;
    for (;;) {
      status = await connection.getSignatureStatus(signature);
      if (status !== 'SignatureNotFound') {
        break;
      }

      if (--statusRetries <= 0) {
        break;
      }
      await sleep(500);
    }

    if (status === 'Confirmed') {
      break;
    }
    if (--sendRetries <= 0) {
      const duration = (Date.now() - start) / 1000;
      throw new Error(
        `Transaction '${signature}' was not confirmed in ${duration.toFixed(
          2,
        )} seconds (${status})`,
      );
    }

    if (status !== 'AccountInUse' && status !== 'SignatureNotFound') {
      throw new Error(`Transaction ${signature} failed (${status})`);
    }

    // Retry in 0..100ms to try to avoid another AccountInUse collision
    await sleep(Math.random() * 100);
  }

  invariant(signature !== undefined);
  return signature;
}
