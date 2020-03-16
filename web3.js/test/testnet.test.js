// @flow
import {testnetChannelEndpoint} from '../src/util/testnet';

test('invalid', () => {
  expect(() => {
    testnetChannelEndpoint('abc123');
  }).toThrow();
});

test('stable', () => {
  expect(testnetChannelEndpoint('stable')).toEqual('https://devnet.solana.com');

  expect(testnetChannelEndpoint('stable', true)).toEqual(
    'https://devnet.solana.com',
  );

  expect(testnetChannelEndpoint('stable', false)).toEqual(
    'http://devnet.solana.com',
  );
});

test('default', () => {
  testnetChannelEndpoint(); // Should not throw
});
