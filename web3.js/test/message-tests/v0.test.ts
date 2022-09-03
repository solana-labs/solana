import {expect} from 'chai';

import {MessageV0} from '../../src/message';
import {PublicKey} from '../../src/publickey';

describe('MessageV0', () => {
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
