// @flow

import invariant from 'assert';

import {Connection} from '../connection';
import type {Commitment} from '../connection';
import {Transaction} from '../transaction';
import {sleep} from './sleep';
import type {Account} from '../account';
import type {TransactionSignature} from '../transaction';
import {DEFAULT_TICKS_PER_SLOT, NUM_TICKS_PER_SECOND} from '../timing';

const MS_PER_SECOND = 1000;
const MS_PER_SLOT =
  (DEFAULT_TICKS_PER_SLOT / NUM_TICKS_PER_SECOND) * MS_PER_SECOND;

const NUM_SEND_RETRIES = 10;
const NUM_STATUS_RETRIES = 10;

/**
 * Sign, send and confirm a transaction with recent commitment level
 */
export async function sendAndConfirmRecentTransaction(
  connection: Connection,
  transaction: Transaction,
  ...signers: Array<Account>
): Promise<TransactionSignature> {
  return await _sendAndConfirmTransaction(
    connection,
    transaction,
    signers,
    'recent',
  );
}

/**
 * Sign, send and confirm a transaction
 */
export async function sendAndConfirmTransaction(
  connection: Connection,
  transaction: Transaction,
  ...signers: Array<Account>
): Promise<TransactionSignature> {
  return await _sendAndConfirmTransaction(connection, transaction, signers);
}

async function _sendAndConfirmTransaction(
  connection: Connection,
  transaction: Transaction,
  signers: Array<Account>,
  commitment: ?Commitment,
): Promise<TransactionSignature> {
  const statusCommitment = commitment || connection.commitment || 'max';

  let sendRetries = NUM_SEND_RETRIES;
  let signature;

  for (;;) {
    const start = Date.now();
    signature = await connection.sendTransaction(transaction, ...signers);

    // Wait up to a couple slots for a confirmation
    let status = null;
    let statusRetries = NUM_STATUS_RETRIES;
    for (;;) {
      status = (await connection.getSignatureStatus(signature)).value;
      if (status) {
        // Recieved a status, if not an error wait for confirmation
        statusRetries = NUM_STATUS_RETRIES;
        if (
          status.err ||
          status.confirmations === null ||
          (statusCommitment === 'recent' && status.confirmations >= 1)
        ) {
          break;
        }
      }

      if (--statusRetries <= 0) {
        break;
      }
      // Sleep for approximately half a slot
      await sleep(MS_PER_SLOT / 2);
    }

    if (status) {
      if (!status.err) {
        break;
      } else if (!('AccountInUse' in status.err)) {
        throw new Error(
          `Transaction ${signature} failed (${JSON.stringify(status)})`,
        );
      }
    }

    if (--sendRetries <= 0) {
      const duration = (Date.now() - start) / 1000;
      throw new Error(
        `Transaction '${signature}' was not confirmed in ${duration.toFixed(
          2,
        )} seconds (${JSON.stringify(status)})`,
      );
    }

    // Retry in 0..100ms to try to avoid another AccountInUse collision
    await sleep(Math.random() * 100);
  }

  invariant(signature !== undefined);
  return signature;
}
