// @flow
import {clusterApiUrl} from '../src/util/cluster';

test('invalid', () => {
  expect(() => {
    // $FlowIgnore
    clusterApiUrl('abc123');
  }).toThrow();
});

test('devnet', () => {
  expect(clusterApiUrl()).toEqual('https://devnet.solana.com');
  expect(clusterApiUrl('devnet')).toEqual('https://devnet.solana.com');
  expect(clusterApiUrl('devnet', true)).toEqual('https://devnet.solana.com');
  expect(clusterApiUrl('devnet', false)).toEqual('http://devnet.solana.com');
});
