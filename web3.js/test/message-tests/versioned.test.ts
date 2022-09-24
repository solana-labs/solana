import {expect} from 'chai';

import {VersionedMessage} from '../../src/message';

describe('VersionedMessage', () => {
  it('deserializeMessageVersion', () => {
    const bufferWithLegacyPrefix = new Uint8Array([1]);
    expect(
      VersionedMessage.deserializeMessageVersion(bufferWithLegacyPrefix),
    ).to.eq('legacy');

    for (const version of [0, 1, 127]) {
      const bufferWithVersionPrefix = new Uint8Array([(1 << 7) + version]);
      expect(
        VersionedMessage.deserializeMessageVersion(bufferWithVersionPrefix),
      ).to.eq(version);
    }
  });

  it('deserialize failure', () => {
    const bufferWithV1Prefix = new Uint8Array([(1 << 7) + 1]);
    expect(() => {
      VersionedMessage.deserialize(bufferWithV1Prefix);
    }).to.throw(
      'Transaction message version 1 deserialization is not supported',
    );
  });
});
