// @flow
import {clusterApiUrl} from '../src/util/cluster';
import {expect} from 'chai';

describe('Cluster Util', () => {
  it('invalid', () => {
    expect(() => {
      // $FlowExpectedError
      clusterApiUrl('abc123');
    }).to.throw();
  });

  it('devnet', () => {
    expect(clusterApiUrl()).to.eq('https://devnet.solana.com');
    expect(clusterApiUrl('devnet')).to.eq('https://devnet.solana.com');
    expect(clusterApiUrl('devnet', true)).to.eq('https://devnet.solana.com');
    expect(clusterApiUrl('devnet', false)).to.eq('http://devnet.solana.com');
  });
});
