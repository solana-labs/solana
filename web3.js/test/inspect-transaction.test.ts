import {expect} from 'chai';
import * as bs58 from 'bs58';

import {Transaction} from '../src/transaction';
import {clusterApiUrl} from '../src/util/cluster';
import {
  getInspectTransactionUrl,
  getExplorerUrlWithCluster,
} from '../src/util/inspect-transaction';

describe('Inspect Transaction', () => {
  const transaction = Transaction.from(
    Buffer.from(
      'AVuErQHaXv0SG0/PchunfxHKt8wMRfMZzqV0tkC5qO6owYxWU2v871AoWywGoFQr4z+q/7mE8lIufNl/kxj+nQ0BAAEDE5j2LG0aRXxRumpLXz29L2n8qTIWIY3ImX5Ba9F9k8r9Q5/Mtmcn8onFxt47xKj+XdXXd3C8j/FcPu7csUrz/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAxJrndgN4IFTxep3s6kO0ROug7bEsbx0xxuDkqEvwUusBAgIAAQwCAAAAMQAAAAAAAAA=',
      'base64',
    ),
  );

  describe('resolves explorer url', () => {
    describe('resolves named clusters', () => {
      it('does not specify cluster for mainnet-beta', () => {
        expect(getExplorerUrlWithCluster('/', 'mainnet-beta').href).to.eq(
          'https://explorer.solana.com/',
        );
      });
      it('devnet and testnet adds query parameter', () => {
        expect(getExplorerUrlWithCluster('/', 'devnet').href).to.eq(
          'https://explorer.solana.com/?cluster=devnet',
        );
        expect(getExplorerUrlWithCluster('/', 'testnet').href).to.eq(
          'https://explorer.solana.com/?cluster=testnet',
        );
      });
    });

    describe('resolves default cluster urls', () => {
      it('mainnet-beta', () => {
        expect(
          getExplorerUrlWithCluster('/', clusterApiUrl('mainnet-beta', true))
            .href,
        ).to.eq('https://explorer.solana.com/');
      });
      it('devnet and testnet', () => {
        expect(
          getExplorerUrlWithCluster('/', clusterApiUrl('devnet', true)).href,
        ).to.eq('https://explorer.solana.com/?cluster=devnet');
        expect(
          getExplorerUrlWithCluster('/', clusterApiUrl('testnet', true)).href,
        ).to.eq('https://explorer.solana.com/?cluster=testnet');
      });
    });

    it('resolves custom rpc urls', () => {
      expect(
        getExplorerUrlWithCluster('/', 'https://solana-rpc.example.com').href,
      ).to.eq(
        'https://explorer.solana.com/?cluster=custom&customUrl=https%3A%2F%2Fsolana-rpc.example.com',
      );
    });
  });

  it('inspects transaction signatures', () => {
    expect(getInspectTransactionUrl(bs58.encode(transaction.signature!))).to.eq(
      'https://explorer.solana.com/tx/2q8Fs6wmDHcdNKJnVJoXYnDVqrWm8GYsxYt2QwUPvadkEQXinWJRwWVzxdUmJY9YH8iZETcXjk3ZdBcDUkuUtfKz/inspect',
    );
  });

  it('inspects encoded transactions', () => {
    expect(getInspectTransactionUrl(transaction)).to.eq(
      'https://explorer.solana.com/tx/inspector?message=AVuErQHaXv0SG0%2FPchunfxHKt8wMRfMZzqV0tkC5qO6owYxWU2v871AoWywGoFQr4z%2Bq%2F7mE8lIufNl%2Fkxj%2BnQ0BAAEDE5j2LG0aRXxRumpLXz29L2n8qTIWIY3ImX5Ba9F9k8r9Q5%2FMtmcn8onFxt47xKj%2BXdXXd3C8j%2FFcPu7csUrz%2FAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAxJrndgN4IFTxep3s6kO0ROug7bEsbx0xxuDkqEvwUusBAgIAAQwCAAAAMQAAAAAAAAA%3D',
    );
  });
});
