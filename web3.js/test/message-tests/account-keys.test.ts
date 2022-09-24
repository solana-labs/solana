import {expect} from 'chai';

import {
  MessageAccountKeys,
  MessageCompiledInstruction,
} from '../../src/message';
import {PublicKey} from '../../src/publickey';
import {TransactionInstruction} from '../../src/transaction';

function createTestKeys(count: number): Array<PublicKey> {
  return new Array(count).fill(0).map(() => PublicKey.unique());
}

describe('MessageAccountKeys', () => {
  it('keySegments', () => {
    const keys = createTestKeys(6);
    const staticAccountKeys = keys.slice(0, 3);
    const accountKeysFromLookups = {
      writable: [keys[3], keys[4]],
      readonly: [keys[5]],
    };

    const accountKeys = new MessageAccountKeys(
      staticAccountKeys,
      accountKeysFromLookups,
    );

    const expectedSegments = [
      staticAccountKeys,
      accountKeysFromLookups.writable,
      accountKeysFromLookups.readonly,
    ];

    expect(expectedSegments).to.eql(accountKeys.keySegments());
  });

  it('get', () => {
    const keys = createTestKeys(3);
    const accountKeys = new MessageAccountKeys(keys);

    expect(accountKeys.get(0)).to.eq(keys[0]);
    expect(accountKeys.get(1)).to.eq(keys[1]);
    expect(accountKeys.get(2)).to.eq(keys[2]);
    expect(accountKeys.get(3)).to.be.undefined;
  });

  it('get with loaded addresses', () => {
    const keys = createTestKeys(6);
    const staticAccountKeys = keys.slice(0, 3);
    const accountKeysFromLookups = {
      writable: [keys[3], keys[4]],
      readonly: [keys[5]],
    };

    const accountKeys = new MessageAccountKeys(
      staticAccountKeys,
      accountKeysFromLookups,
    );

    expect(accountKeys.get(0)).to.eq(keys[0]);
    expect(accountKeys.get(1)).to.eq(keys[1]);
    expect(accountKeys.get(2)).to.eq(keys[2]);
    expect(accountKeys.get(3)).to.eq(keys[3]);
    expect(accountKeys.get(4)).to.eq(keys[4]);
    expect(accountKeys.get(5)).to.eq(keys[5]);
  });

  it('length', () => {
    const keys = createTestKeys(6);
    const accountKeys = new MessageAccountKeys(keys);
    expect(accountKeys.length).to.eq(6);
  });

  it('length with loaded addresses', () => {
    const keys = createTestKeys(6);
    const accountKeys = new MessageAccountKeys(keys.slice(0, 3), {
      writable: [],
      readonly: keys.slice(3, 6),
    });

    expect(accountKeys.length).to.eq(6);
  });

  it('compileInstructions', () => {
    const keys = createTestKeys(3);
    const staticAccountKeys = [keys[0]];
    const accountKeysFromLookups = {
      writable: [keys[1]],
      readonly: [keys[2]],
    };

    const accountKeys = new MessageAccountKeys(
      staticAccountKeys,
      accountKeysFromLookups,
    );

    const instruction = new TransactionInstruction({
      programId: keys[0],
      keys: [
        {
          pubkey: keys[1],
          isSigner: true,
          isWritable: true,
        },
        {
          pubkey: keys[2],
          isSigner: true,
          isWritable: true,
        },
      ],
      data: Buffer.alloc(0),
    });

    const expectedInstruction: MessageCompiledInstruction = {
      programIdIndex: 0,
      accountKeyIndexes: [1, 2],
      data: new Uint8Array(0),
    };

    expect(accountKeys.compileInstructions([instruction])).to.eql([
      expectedInstruction,
    ]);
  });

  it('compileInstructions with unknown key', () => {
    const keys = createTestKeys(3);
    const staticAccountKeys = [keys[0]];
    const accountKeysFromLookups = {
      writable: [keys[1]],
      readonly: [keys[2]],
    };

    const accountKeys = new MessageAccountKeys(
      staticAccountKeys,
      accountKeysFromLookups,
    );

    const unknownKey = PublicKey.unique();
    const testInstructions = [
      new TransactionInstruction({
        programId: unknownKey,
        keys: [],
        data: Buffer.alloc(0),
      }),
      new TransactionInstruction({
        programId: keys[0],
        keys: [
          {
            pubkey: keys[1],
            isSigner: true,
            isWritable: true,
          },
          {
            pubkey: unknownKey,
            isSigner: true,
            isWritable: true,
          },
        ],
        data: Buffer.alloc(0),
      }),
    ];

    for (const instruction of testInstructions) {
      expect(() => accountKeys.compileInstructions([instruction])).to.throw(
        'Encountered an unknown instruction account key during compilation',
      );
    }
  });

  it('compileInstructions with too many account keys', () => {
    const keys = createTestKeys(257);
    const accountKeys = new MessageAccountKeys(keys.slice(0, 256), {
      writable: [keys[256]],
      readonly: [],
    });
    expect(() => accountKeys.compileInstructions([])).to.throw(
      'Account index overflow encountered during compilation',
    );
  });
});
