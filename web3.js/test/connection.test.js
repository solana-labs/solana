// @flow
import {Connection} from '../src/connection';
import {Account} from '../src/account';


const url = 'http://localhost:8899';
//const url = 'http://master.testnet.solana.com:8899';

test.skip('get balance', async () => {
  const account = new Account();
  const connection = new Connection(url);

  const balance = await connection.getBalance(account.publicKey);
  expect(balance).toBeGreaterThanOrEqual(0);
});

test.skip('throws on bad transaction', () => {
  const connection = new Connection(url);

  expect(connection.confirmTransaction('bad transaction signature'))
  .rejects.toThrow('Invalid request');
});

test.skip('get transaction count', async () => {
  const connection = new Connection(url);

  const count = await connection.getTransactionCount();
  expect(count).toBeGreaterThanOrEqual(0);
});

test.skip('get last Id', async () => {
  const connection = new Connection(url);

  const lastId = await connection.getLastId();
  expect(lastId.length).toBeGreaterThanOrEqual(43);
});

test.skip('get finality', async () => {
  const connection = new Connection(url);

  const finality = await connection.getFinality();
  expect(finality).toBeGreaterThanOrEqual(0);
});

test.skip('request airdrop', async () => {
  const account = new Account();
  const connection = new Connection(url);

  await Promise.all([
    connection.requestAirdrop(account.publicKey, 40),
    connection.requestAirdrop(account.publicKey, 2),
  ]);

  const balance = await connection.getBalance(account.publicKey);
  expect(balance).toBe(42);
});


