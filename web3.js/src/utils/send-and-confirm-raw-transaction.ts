import type {Buffer} from 'buffer';

import {BlockhashWithExpiryBlockHeight, Connection} from '../connection';
import type {TransactionSignature} from '../transaction';
import type {ConfirmOptions} from '../connection';

/**
 * Send and confirm a raw transaction
 *
 * If `commitment` option is not specified, defaults to 'max' commitment.
 *
 * @param {Connection} connection
 * @param {Buffer} rawTransaction
 * @param {BlockhashWithExpiryBlockHeight} blockhashWithExpiryBlockHeight
 * @param {ConfirmOptions} [options]
 * @returns {Promise<TransactionSignature>}
 */
export async function sendAndConfirmRawTransaction(
  connection: Connection,
  rawTransaction: Buffer,
  blockhashWithExpiryBlockHeight: BlockhashWithExpiryBlockHeight,
  options?: ConfirmOptions,
): Promise<TransactionSignature>;

/**
 * @deprecated Calling `sendAndConfirmRawTransaction()` without a `blockhashWithExpiryBlockHeight`
 * is no longer supported and will be removed in a future version.
 */
// eslint-disable-next-line no-redeclare
export async function sendAndConfirmRawTransaction(
  connection: Connection,
  rawTransaction: Buffer,
  options?: ConfirmOptions,
): Promise<TransactionSignature>;

// eslint-disable-next-line no-redeclare
export async function sendAndConfirmRawTransaction(
  connection: Connection,
  rawTransaction: Buffer,
  blockhashWithExpiryBlockHeightOrConfirmOptions:
    | BlockhashWithExpiryBlockHeight
    | ConfirmOptions
    | undefined,
  maybeConfirmOptions?: ConfirmOptions,
): Promise<TransactionSignature> {
  let blockhashWithExpiryBlockHeight:
    | BlockhashWithExpiryBlockHeight
    | undefined;
  let options: ConfirmOptions | undefined;
  if (
    blockhashWithExpiryBlockHeightOrConfirmOptions &&
    Object.prototype.hasOwnProperty.call(
      blockhashWithExpiryBlockHeightOrConfirmOptions,
      'lastValidBlockHeight',
    )
  ) {
    blockhashWithExpiryBlockHeight =
      blockhashWithExpiryBlockHeightOrConfirmOptions as BlockhashWithExpiryBlockHeight;
    options = maybeConfirmOptions;
  } else {
    options = blockhashWithExpiryBlockHeightOrConfirmOptions as
      | ConfirmOptions
      | undefined;
  }
  const sendOptions = options && {
    skipPreflight: options.skipPreflight,
    preflightCommitment: options.preflightCommitment || options.commitment,
    minContextSlot: options.minContextSlot,
  };

  const signature = await connection.sendRawTransaction(
    rawTransaction,
    sendOptions,
  );

  const commitment = options && options.commitment;
  const confirmationPromise = blockhashWithExpiryBlockHeight
    ? connection.confirmTransaction(
        {...blockhashWithExpiryBlockHeight, signature},
        commitment,
      )
    : connection.confirmTransaction(signature, commitment);
  const status = (await confirmationPromise).value;

  if (status.err) {
    throw new Error(
      `Raw transaction ${signature} failed (${JSON.stringify(status)})`,
    );
  }

  return signature;
}
