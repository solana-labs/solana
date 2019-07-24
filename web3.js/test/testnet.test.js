// @flow
import {testnetChannelEndpoint} from '../src/util/testnet';

test('invalid', () => {
  expect(() => {
    testnetChannelEndpoint('abc123');
  }).toThrow();
});

test('edge', () => {
  expect(testnetChannelEndpoint('edge')).toEqual(
    'https://edge.testnet.solana.com:8443',
  );
});

test('default', () => {
  testnetChannelEndpoint(); // Should not throw
});
