import bs58 from 'bs58';
import {expect} from 'chai';
import {sha256} from '@noble/hashes/sha256';

import {MessageV0} from '../../src/message';
import {TransactionInstruction} from '../../src/transaction';
import {PublicKey} from '../../src/publickey';
import {AddressLookupTableAccount} from '../../src/programs';

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

describe('MessageV0', () => {
  it('compile', () => {
    const keys = createTestKeys(7);
    const recentBlockhash = bs58.encode(sha256('test'));
    const payerKey = keys[0];
    const instructions = [
      new TransactionInstruction({
        programId: keys[4],
        keys: [
          {pubkey: keys[1], isSigner: true, isWritable: true},
          {pubkey: keys[2], isSigner: false, isWritable: false},
          {pubkey: keys[3], isSigner: false, isWritable: false},
        ],
        data: Buffer.alloc(1),
      }),
      new TransactionInstruction({
        programId: keys[1],
        keys: [
          {pubkey: keys[2], isSigner: true, isWritable: false},
          {pubkey: keys[3], isSigner: false, isWritable: true},
        ],
        data: Buffer.alloc(2),
      }),
      new TransactionInstruction({
        programId: keys[3],
        keys: [
          {pubkey: keys[5], isSigner: false, isWritable: true},
          {pubkey: keys[6], isSigner: false, isWritable: false},
        ],
        data: Buffer.alloc(3),
      }),
    ];

    const lookupTable = createTestLookupTable(keys);
    const message = MessageV0.compile({
      payerKey,
      recentBlockhash,
      instructions,
      addressLookupTableAccounts: [lookupTable],
    });

    expect(message.staticAccountKeys).to.eql([
      payerKey, // payer is first
      keys[1], // other writable signer
      keys[2], // sole readonly signer
      keys[3], // sole writable non-signer
      keys[4], // sole readonly non-signer
    ]);
    expect(message.header).to.eql({
      numRequiredSignatures: 3,
      numReadonlySignedAccounts: 1,
      numReadonlyUnsignedAccounts: 1,
    });
    // only keys 5 and 6 are eligible to be referenced by a lookup table
    // because they are not invoked and are not signers
    expect(message.addressTableLookups).to.eql([
      {
        accountKey: lookupTable.key,
        writableIndexes: [5],
        readonlyIndexes: [6],
      },
    ]);
    expect(message.compiledInstructions).to.eql([
      {
        programIdIndex: 4,
        accountKeyIndexes: [1, 2, 3],
        data: new Uint8Array(1),
      },
      {
        programIdIndex: 1,
        accountKeyIndexes: [2, 3],
        data: new Uint8Array(2),
      },
      {
        programIdIndex: 3,
        accountKeyIndexes: [5, 6],
        data: new Uint8Array(3),
      },
    ]);
    expect(message.recentBlockhash).to.eq(recentBlockhash);
  });

  it('serialize and deserialize', () => {
    const messageV0 = new MessageV0({
      header: {
        numRequiredSignatures: 1,
        numReadonlySignedAccounts: 0,
        numReadonlyUnsignedAccounts: 1,
      },
      staticAccountKeys: [new PublicKey(1), new PublicKey(2)],
      compiledInstructions: [
        {
          programIdIndex: 1,
          accountKeyIndexes: [2, 3],
          data: new Uint8Array(10),
        },
      ],
      recentBlockhash: new PublicKey(0).toString(),
      addressTableLookups: [
        {
          accountKey: new PublicKey(3),
          writableIndexes: [1],
          readonlyIndexes: [],
        },
        {
          accountKey: new PublicKey(4),
          writableIndexes: [],
          readonlyIndexes: [2],
        },
      ],
    });
    const serializedMessage = messageV0.serialize();
    const deserializedMessage = MessageV0.deserialize(serializedMessage);
    expect(JSON.stringify(messageV0)).to.eql(
      JSON.stringify(deserializedMessage),
    );
  });

  it('deserialize failures', () => {
    const bufferWithLegacyPrefix = new Uint8Array([1]);
    expect(() => {
      MessageV0.deserialize(bufferWithLegacyPrefix);
    }).to.throw('Expected versioned message but received legacy message');

    const bufferWithV1Prefix = new Uint8Array([(1 << 7) + 1]);
    expect(() => {
      MessageV0.deserialize(bufferWithV1Prefix);
    }).to.throw(
      'Expected versioned message with version 0 but found version 1',
    );
  });
});
