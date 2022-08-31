import bs58 from 'bs58';
import {expect} from 'chai';
import {sha256} from '@noble/hashes/sha256';

import {
  TransactionInstruction,
  TransactionMessage,
} from '../../src/transaction';
import {PublicKey} from '../../src/publickey';
import {AddressLookupTableAccount} from '../../src/programs';
import {MessageV0} from '../../src/message';

function createTestKeys(count: number): Array<PublicKey> {
  return new Array(count).fill(0).map(() => PublicKey.unique());
}

function createTestLookupTable(
  addresses: Array<PublicKey>,
): AddressLookupTableAccount {
  const U64_MAX = 2n ** 64n - 1n;
  return new AddressLookupTableAccount({
    key: PublicKey.unique(),
    state: {
      lastExtendedSlot: 0,
      lastExtendedSlotStartIndex: 0,
      deactivationSlot: U64_MAX,
      authority: PublicKey.unique(),
      addresses,
    },
  });
}

describe('TransactionMessage', () => {
  it('decompile', () => {
    const keys = createTestKeys(7);
    const recentBlockhash = bs58.encode(sha256('test'));
    const payerKey = keys[0];
    const instructions = [
      new TransactionInstruction({
        programId: keys[4],
        keys: [
          {pubkey: keys[1], isSigner: true, isWritable: true},
          {pubkey: keys[2], isSigner: true, isWritable: false},
          {pubkey: keys[3], isSigner: false, isWritable: true},
          {pubkey: keys[5], isSigner: false, isWritable: true},
          {pubkey: keys[6], isSigner: false, isWritable: false},
        ],
        data: Buffer.alloc(1),
      }),
      new TransactionInstruction({
        programId: keys[1],
        keys: [],
        data: Buffer.alloc(2),
      }),
      new TransactionInstruction({
        programId: keys[3],
        keys: [],
        data: Buffer.alloc(3),
      }),
    ];

    const addressLookupTableAccounts = [createTestLookupTable(keys)];
    const message = MessageV0.compile({
      payerKey,
      recentBlockhash,
      instructions,
      addressLookupTableAccounts,
    });

    expect(() => TransactionMessage.decompile(message)).to.throw(
      'Failed to find key',
    );

    const accountKeys = message.getAccountKeys({addressLookupTableAccounts});
    const decompiledMessage = TransactionMessage.decompile(message, {
      addressLookupTableAccounts,
    });

    expect(decompiledMessage.accountKeys).to.eql(accountKeys);
    expect(decompiledMessage.recentBlockhash).to.eq(recentBlockhash);
    expect(decompiledMessage.instructions).to.eql(instructions);

    expect(decompiledMessage).to.eql(
      TransactionMessage.decompile(message, {
        accountKeysFromLookups: accountKeys.accountKeysFromLookups!,
      }),
    );
  });
});
