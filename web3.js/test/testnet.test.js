// @flow
import {testnetChannelEndpoint} from '../src/util/testnet';

test('invalid', () => {
  expect(() => {
    testnetChannelEndpoint('abc123');
  }).toThrow();
});

test('edge', () => {
  expect(testnetChannelEndpoint('edge')).toEqual(
    'https://api.edge.testnet.solana.com',
  );
});

test('default', () => {
  testnetChannelEndpoint(); // Should not throw
});
