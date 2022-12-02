import {Connection, SignatureResult} from '../connection';
import {Transaction} from '../transaction';
import type {ConfirmOptions} from '../connection';
import type {Signer} from '../keypair';
import type {TransactionSignature} from '../transaction';

/**
 * Sign, send and confirm a transaction.
 *
 * If `commitment` option is not specified, defaults to 'max' commitment.
 *
 * @param {Connection} connection
 * @param {Transaction} transaction
 * @param {Array<Signer>} signers
 * @param {ConfirmOptions} [options]
 * @returns {Promise<TransactionSignature>}
 */
export async function sendAndConfirmTransaction(
  connection: Connection,
  transaction: Transaction,
  signers: Array<Signer>,
  options?: ConfirmOptions,
): Promise<TransactionSignature> {
  const sendOptions = options && {
    skipPreflight: options.skipPreflight,
    preflightCommitment: options.preflightCommitment || options.commitment,
    maxRetries: options.maxRetries,
    minContextSlot: options.minContextSlot,
  };

  const signature = await connection.sendTransaction(
    transaction,
    signers,
    sendOptions,
  );

  let status: SignatureResult;
  if (
    transaction.recentBlockhash != null &&
    transaction.lastValidBlockHeight != null
  ) {
    status = (
      await connection.confirmTransaction(
        {
          signature: signature,
          blockhash: transaction.recentBlockhash,
          lastValidBlockHeight: transaction.lastValidBlockHeight,
        },
        options && options.commitment,
      )
    ).value;
  } else if (
    transaction.minNonceContextSlot != null &&
    transaction.nonceInfo != null
  ) {
    const {nonceInstruction} = transaction.nonceInfo;
    const nonceAccountPubkey = nonceInstruction.keys[0].pubkey;
    status = (
      await connection.confirmTransaction(
        {
          minContextSlot: transaction.minNonceContextSlot,
          nonceAccountPubkey,
          nonceValue: transaction.nonceInfo.nonce,
          signature,
        },
        options && options.commitment,
      )
    ).value;
  } else {
    status = (
      await connection.confirmTransaction(
        signature,
        options && options.commitment,
      )
    ).value;
  }

  if (status.err) {
    throw new Error(
      `Transaction ${signature} failed (${JSON.stringify(status)})`,
    );
  }

  return signature;
}
