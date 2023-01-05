import {AccountKeysFromLookups} from '../message/account-keys';
import assert from '../utils/assert';
import {toBuffer} from '../utils/to-buffer';
import {Blockhash} from '../blockhash';
import {Message, MessageV0, VersionedMessage} from '../message';
import {PublicKey} from '../publickey';
import {AddressLookupTableAccount} from '../programs';
import {AccountMeta, TransactionInstruction} from './legacy';

export type TransactionMessageArgs = {
  payerKey: PublicKey;
  instructions: Array<TransactionInstruction>;
  recentBlockhash: Blockhash;
};

export type DecompileArgs =
  | {
      accountKeysFromLookups: AccountKeysFromLookups;
    }
  | {
      addressLookupTableAccounts: AddressLookupTableAccount[];
    };

export class TransactionMessage {
  payerKey: PublicKey;
  instructions: Array<TransactionInstruction>;
  recentBlockhash: Blockhash;

  constructor(args: TransactionMessageArgs) {
    this.payerKey = args.payerKey;
    this.instructions = args.instructions;
    this.recentBlockhash = args.recentBlockhash;
  }

  static decompile(
    message: VersionedMessage,
    args?: DecompileArgs,
  ): TransactionMessage {
    const {header, compiledInstructions, recentBlockhash} = message;

    const {
      numRequiredSignatures,
      numReadonlySignedAccounts,
      numReadonlyUnsignedAccounts,
    } = header;

    const numWritableSignedAccounts =
      numRequiredSignatures - numReadonlySignedAccounts;
    assert(numWritableSignedAccounts > 0, 'Message header is invalid');

    const numWritableUnsignedAccounts =
      message.staticAccountKeys.length -
      numRequiredSignatures -
      numReadonlyUnsignedAccounts;
    assert(numWritableUnsignedAccounts >= 0, 'Message header is invalid');

    const accountKeys = message.getAccountKeys(args);
    const payerKey = accountKeys.get(0);
    if (payerKey === undefined) {
      throw new Error(
        'Failed to decompile message because no account keys were found',
      );
    }

    const instructions: TransactionInstruction[] = [];
    for (const compiledIx of compiledInstructions) {
      const keys: AccountMeta[] = [];

      for (const keyIndex of compiledIx.accountKeyIndexes) {
        const pubkey = accountKeys.get(keyIndex);
        if (pubkey === undefined) {
          throw new Error(
            `Failed to find key for account key index ${keyIndex}`,
          );
        }

        const isSigner = keyIndex < numRequiredSignatures;

        let isWritable;
        if (isSigner) {
          isWritable = keyIndex < numWritableSignedAccounts;
        } else if (keyIndex < accountKeys.staticAccountKeys.length) {
          isWritable =
            keyIndex - numRequiredSignatures < numWritableUnsignedAccounts;
        } else {
          isWritable =
            keyIndex - accountKeys.staticAccountKeys.length <
            // accountKeysFromLookups cannot be undefined because we already found a pubkey for this index above
            accountKeys.accountKeysFromLookups!.writable.length;
        }

        keys.push({
          pubkey,
          isSigner: keyIndex < header.numRequiredSignatures,
          isWritable,
        });
      }

      const programId = accountKeys.get(compiledIx.programIdIndex);
      if (programId === undefined) {
        throw new Error(
          `Failed to find program id for program id index ${compiledIx.programIdIndex}`,
        );
      }

      instructions.push(
        new TransactionInstruction({
          programId,
          data: toBuffer(compiledIx.data),
          keys,
        }),
      );
    }

    return new TransactionMessage({
      payerKey,
      instructions,
      recentBlockhash,
    });
  }

  compileToLegacyMessage(): Message {
    return Message.compile({
      payerKey: this.payerKey,
      recentBlockhash: this.recentBlockhash,
      instructions: this.instructions,
    });
  }

  compileToV0Message(
    addressLookupTableAccounts?: AddressLookupTableAccount[],
  ): MessageV0 {
    return MessageV0.compile({
      payerKey: this.payerKey,
      recentBlockhash: this.recentBlockhash,
      instructions: this.instructions,
      addressLookupTableAccounts,
    });
  }
}
