import {expect} from 'chai';

import {clusterApiUrl} from '../src/util/cluster';

describe('Cluster Util', () => {
  it('invalid', () => {
    expect(() => {
      clusterApiUrl('abc123' as any);
    }).to.throw();
  });

  it('devnet', () => {
    expect(clusterApiUrl()).to.eq('https://devnet.solana.com');
    expect(clusterApiUrl('devnet')).to.eq('https://devnet.solana.com');
    expect(clusterApiUrl('devnet', true)).to.eq('https://devnet.solana.com');
    expect(clusterApiUrl('devnet', false)).to.eq('http://devnet.solana.com');
  });
});
