import bs58 from 'bs58';
import {expect} from 'chai';
import {sha256} from '@noble/hashes/sha256';

import {
  Transaction,
  TransactionInstruction,
  TransactionMessage,
} from '../../src/transaction';
import {PublicKey} from '../../src/publickey';
import {AddressLookupTableAccount} from '../../src/programs';
import {Message, MessageV0} from '../../src/message';

function createTestKeys(count: number): Array<PublicKey> {
  return new Array(count).fill(0).map(() => PublicKey.unique());
}

function createTestLookupTable(
  addresses: Array<PublicKey>,
): AddressLookupTableAccount {
  const U64_MAX = BigInt('0xffffffffffffffff');
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
  it('decompiles a legacy message', () => {
    const keys = createTestKeys(7);
    const recentBlockhash = bs58.encode(sha256('test'));
    const payerKey = keys[0];
    const instructions = [
      new TransactionInstruction({
        programId: keys[5],
        keys: [
          {pubkey: keys[0], isSigner: true, isWritable: true},
          {pubkey: keys[6], isSigner: false, isWritable: false},
          {pubkey: keys[1], isSigner: false, isWritable: true},
          {pubkey: keys[3], isSigner: false, isWritable: false},
          {pubkey: keys[4], isSigner: false, isWritable: false},
          {pubkey: keys[2], isSigner: false, isWritable: false},
        ],
        data: Buffer.alloc(1),
      }),
    ];

    const message = Message.compile({
      instructions,
      payerKey,
      recentBlockhash,
    });

    expect(() => TransactionMessage.decompile(message)).not.to.throw(
      'Failed to get account keys because address table lookups were not resolved',
    );

    const decompiledMessage = TransactionMessage.decompile(message);

    expect(decompiledMessage.payerKey).to.eql(payerKey);
    expect(decompiledMessage.recentBlockhash).to.eq(recentBlockhash);
    expect(decompiledMessage.instructions).to.eql(instructions);
  });

  // Regression test for https://github.com/solana-labs/solana/issues/28900
  it('decompiles a legacy message the same way as the old API', () => {
    const accountKeys = createTestKeys(7);
    const legacyMessage = new Message({
      header: {
        numRequiredSignatures: 1,
        numReadonlySignedAccounts: 0,
        numReadonlyUnsignedAccounts: 5,
      },
      recentBlockhash: bs58.encode(sha256('test')),
      accountKeys,
      instructions: [
        {
          accounts: [0, 6, 1, 3, 4, 2],
          data: '',
          programIdIndex: 5,
        },
      ],
    });

    const transactionFromLegacyAPI = Transaction.populate(legacyMessage);
    const transactionMessage = TransactionMessage.decompile(legacyMessage);

    expect(transactionMessage.payerKey).to.eql(
      transactionFromLegacyAPI.feePayer,
    );
    expect(transactionMessage.instructions).to.eql(
      transactionFromLegacyAPI.instructions,
    );
    expect(transactionMessage.recentBlockhash).to.eql(
      transactionFromLegacyAPI.recentBlockhash,
    );
  });

  it('decompiles a V0 message', () => {
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
      'Failed to get account keys because address table lookups were not resolved',
    );

    const accountKeys = message.getAccountKeys({addressLookupTableAccounts});
    const decompiledMessage = TransactionMessage.decompile(message, {
      addressLookupTableAccounts,
    });

    expect(decompiledMessage.payerKey).to.eql(payerKey);
    expect(decompiledMessage.recentBlockhash).to.eq(recentBlockhash);
    expect(decompiledMessage.instructions).to.eql(instructions);

    expect(decompiledMessage).to.eql(
      TransactionMessage.decompile(message, {
        accountKeysFromLookups: accountKeys.accountKeysFromLookups!,
      }),
    );
  });
});
