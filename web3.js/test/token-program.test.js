// @flow

import {
  Connection,
  PublicKey,
  Token,
  TokenAmount,
} from '../src';
import {SYSTEM_TOKEN_PROGRAM_ID} from '../src/token-program';
import {mockRpc, mockRpcEnabled} from './__mocks__/node-fetch';
import {url} from './url';
import {newAccountWithTokens} from './new-account-with-tokens';
import {mockGetLastId} from './mockrpc/getlastid';

if (!mockRpcEnabled) {
  // The default of 5 seconds is too slow for live testing sometimes
  jest.setTimeout(10000);
}

function mockGetSignatureStatus(result: string = 'Confirmed') {
  mockRpc.push([
    url,
    {
      method: 'getSignatureStatus',
    },
    {
      error: null,
      result,
    },
  ]);
}
function mockSendTransaction() {
  mockRpc.push([
    url,
    {
      method: 'sendTransaction',
    },
    {
      error: null,
      result: '3WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    }
  ]);
}


// A token created by the first test and used by all subsequent tests
let testToken: Token;

// Initial owner of the token supply
let initialOwner;
let initialOwnerTokenAccount: PublicKey;

test('create new token', async () => {
  const connection = new Connection(url);

  initialOwner = await newAccountWithTokens(connection);

  {
    // mock SystemProgram.createAccount transaction for Token.createNewToken()
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus();

    // mock Token.newAccount() transaction
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus('SignatureNotFound');
    mockGetSignatureStatus();

    // mock SystemProgram.createAccount transaction for Token.createNewToken()
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus();

    // mock Token.createNewToken() transaction
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus('SignatureNotFound');
    mockGetSignatureStatus();
  }

  [testToken, initialOwnerTokenAccount] = await Token.createNewToken(
    connection,
    initialOwner,
    new TokenAmount(10000),
    'Test token',
    'TEST',
    2
  );

  {
    // mock Token.tokenInfo()'s getAccountInfo
    mockRpc.push([
      url,
      {
        method: 'getAccountInfo',
        params: [testToken.token.toBase58()],
      },
      {
        error: null,
        result: {
          program_id: [...SYSTEM_TOKEN_PROGRAM_ID.toBuffer()],
          tokens: 1,
          userdata: [
            1,
            16, 39, 0, 0, 0, 0, 0, 0,
            2,
            10, 0, 0, 0, 0, 0, 0, 0, 84, 101, 115, 116, 32, 116, 111, 107, 101, 110,
            4, 0, 0, 0, 0, 0, 0, 0, 84, 69, 83, 84
          ],
          executable: false,
          loader_program_id: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
          ],
        }
      }
    ]);
  }

  const tokenInfo = await testToken.tokenInfo();

  expect(tokenInfo.supply.toNumber()).toBe(10000);
  expect(tokenInfo.decimals).toBe(2);
  expect(tokenInfo.name).toBe('Test token');
  expect(tokenInfo.symbol).toBe('TEST');


  {
    // mock Token.accountInfo()'s getAccountInfo
    mockRpc.push([
      url,
      {
        method: 'getAccountInfo',
        params: [initialOwnerTokenAccount.toBase58()],
      },
      {
        error: null,
        result: {
          program_id: [...SYSTEM_TOKEN_PROGRAM_ID.toBuffer()],
          tokens: 1,
          userdata: [
            2,
            ...testToken.token.toBuffer(),
            ...initialOwner.publicKey.toBuffer(),
            16, 39, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
          ],
          executable: false,
          loader_program_id: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
          ],
        }
      }
    ]);
  }

  const accountInfo = await testToken.accountInfo(initialOwnerTokenAccount);

  expect(accountInfo.token.equals(testToken.token)).toBe(true);
  expect(accountInfo.owner.equals(initialOwner.publicKey)).toBe(true);
  expect(accountInfo.amount.toNumber()).toBe(10000);
  expect(accountInfo.source).toBe(null);
  expect(accountInfo.originalAmount.toNumber()).toBe(0);
});


test('create new token account', async () => {
  const connection = new Connection(url);
  const destOwner = await newAccountWithTokens(connection);

  {
    // mock SystemProgram.createAccount transaction for Token.newAccount()
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus();

    // mock Token.newAccount() transaction
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus();
  }

  const dest = await testToken.newAccount(destOwner);
  {
    // mock Token.accountInfo()'s getAccountInfo
    mockRpc.push([
      url,
      {
        method: 'getAccountInfo',
        params: [dest.toBase58()],
      },
      {
        error: null,
        result: {
          program_id: [...SYSTEM_TOKEN_PROGRAM_ID.toBuffer()],
          tokens: 1,
          userdata: [
            2,
            ...testToken.token.toBuffer(),
            ...destOwner.publicKey.toBuffer(),
            0, 0, 0, 0, 0, 0, 0, 0,
            0,
          ],
          executable: false,
          loader_program_id: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
          ],
        }
      }
    ]);
  }

  const accountInfo = await testToken.accountInfo(dest);

  expect(accountInfo.token.equals(testToken.token)).toBe(true);
  expect(accountInfo.owner.equals(destOwner.publicKey)).toBe(true);
  expect(accountInfo.amount.toNumber()).toBe(0);
  expect(accountInfo.source).toBe(null);
});


test('transfer', async () => {
  const connection = new Connection(url);
  const destOwner = await newAccountWithTokens(connection);

  {
    // mock SystemProgram.createAccount transaction for Token.newAccount()
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus();

    // mock Token.newAccount() transaction
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus();
  }

  const dest = await testToken.newAccount(destOwner);

  {
    // mock Token.transfer()'s getAccountInfo
    mockRpc.push([
      url,
      {
        method: 'getAccountInfo',
        params: [initialOwnerTokenAccount.toBase58()],
      },
      {
        error: null,
        result: {
          program_id: [...SYSTEM_TOKEN_PROGRAM_ID.toBuffer()],
          tokens: 1,
          userdata: [
            2,
            ...testToken.token.toBuffer(),
            ...initialOwner.publicKey.toBuffer(),
            123, 0, 0, 0, 0, 0, 0, 0,
            0,
          ],
          executable: false,
          loader_program_id: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
          ],
        }
      }
    ]);

    // mock Token.transfer() transaction
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus();
  }

  await testToken.transfer(
    initialOwner,
    initialOwnerTokenAccount,
    dest,
    123
  );

  {
    // mock Token.accountInfo()'s getAccountInfo
    mockRpc.push([
      url,
      {
        method: 'getAccountInfo',
        params: [dest.toBase58()],
      },
      {
        error: null,
        result: {
          program_id: [...SYSTEM_TOKEN_PROGRAM_ID.toBuffer()],
          tokens: 1,
          userdata: [
            2,
            ...testToken.token.toBuffer(),
            ...dest.toBuffer(),
            123, 0, 0, 0, 0, 0, 0, 0,
            0,
          ],
          executable: false,
          loader_program_id: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
          ],
        }
      }
    ]);
  }

  const destAccountInfo = await testToken.accountInfo(dest);
  expect(destAccountInfo.amount.toNumber()).toBe(123);
});


test('approve/revoke', async () => {
  const connection = new Connection(url);
  const delegateOwner = await newAccountWithTokens(connection);

  {
    // mock SystemProgram.createAccount transaction for Token.newAccount()
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus();

    // mock Token.newAccount() transaction
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus();
  }
  const delegate = await testToken.newAccount(delegateOwner, initialOwnerTokenAccount);

  {
    // mock Token.approve() transaction
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus();
  }

  await testToken.approve(
    initialOwner,
    initialOwnerTokenAccount,
    delegate,
    456
  );

  {
    // mock Token.accountInfo()'s getAccountInfo
    mockRpc.push([
      url,
      {
        method: 'getAccountInfo',
        params: [delegate.toBase58()],
      },
      {
        error: null,
        result: {
          program_id: [...SYSTEM_TOKEN_PROGRAM_ID.toBuffer()],
          tokens: 1,
          userdata: [
            2,
            ...testToken.token.toBuffer(),
            ...delegate.toBuffer(),
            200, 1, 0, 0, 0, 0, 0, 0,
            1,
            ...initialOwnerTokenAccount.toBuffer(),
            200, 1, 0, 0, 0, 0, 0, 0,
          ],
          executable: false,
          loader_program_id: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
          ],
        }
      }
    ]);
  }

  let delegateAccountInfo = await testToken.accountInfo(delegate);

  expect(delegateAccountInfo.amount.toNumber()).toBe(456);
  expect(delegateAccountInfo.originalAmount.toNumber()).toBe(456);
  if (delegateAccountInfo.source === null) {
    throw new Error('source should not be null');
  } else {
    expect(delegateAccountInfo.source.equals(initialOwnerTokenAccount)).toBe(true);
  }

  {
    // mock Token.revoke() transaction
    mockGetLastId();
    mockSendTransaction();
    mockGetSignatureStatus();
  }

  await testToken.revoke(
    initialOwner,
    initialOwnerTokenAccount,
    delegate,
  );

  {
    // mock Token.accountInfo()'s getAccountInfo
    mockRpc.push([
      url,
      {
        method: 'getAccountInfo',
        params: [delegate.toBase58()],
      },
      {
        error: null,
        result: {
          program_id: [...SYSTEM_TOKEN_PROGRAM_ID.toBuffer()],
          tokens: 1,
          userdata: [
            2,
            ...testToken.token.toBuffer(),
            ...delegate.toBuffer(),
            0, 0, 0, 0, 0, 0, 0, 0,
            1,
            ...initialOwnerTokenAccount.toBuffer(),
            0, 0, 0, 0, 0, 0, 0, 0,
          ],
          executable: false,
          loader_program_id: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
          ],
        }
      }
    ]);
  }

  delegateAccountInfo = await testToken.accountInfo(delegate);
  expect(delegateAccountInfo.amount.toNumber()).toBe(0);
  expect(delegateAccountInfo.originalAmount.toNumber()).toBe(0);
  if (delegateAccountInfo.source === null) {
    throw new Error('source should not be null');
  } else {
    expect(delegateAccountInfo.source.equals(initialOwnerTokenAccount)).toBe(true);
  }
});


test('invalid approve', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url);
  const owner = await newAccountWithTokens(connection);

  const account1 = await testToken.newAccount(owner);
  const account1Delegate = await testToken.newAccount(owner, account1);
  const account2 = await testToken.newAccount(owner);

  // account2 is not a delegate account of account1
  await expect(
    testToken.approve(
      owner,
      account1,
      account2,
      123
    )
  ).rejects.toThrow();

  // account1Delegate is not a delegate account of account2
  await expect(
    testToken.approve(
      owner,
      account2,
      account1Delegate,
      123
    )
  ).rejects.toThrow();
});


test('fail on approve overspend', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url);
  const owner = await newAccountWithTokens(connection);

  const account1 = await testToken.newAccount(owner);
  const account1Delegate = await testToken.newAccount(owner, account1);
  const account2 = await testToken.newAccount(owner);

  await testToken.transfer(
    initialOwner,
    initialOwnerTokenAccount,
    account1,
    10,
  );

  await testToken.approve(
    owner,
    account1,
    account1Delegate,
    2
  );

  let delegateAccountInfo = await testToken.accountInfo(account1Delegate);
  expect(delegateAccountInfo.amount.toNumber()).toBe(2);
  expect(delegateAccountInfo.originalAmount.toNumber()).toBe(2);

  await testToken.transfer(
    owner,
    account1Delegate,
    account2,
    1,
  );

  delegateAccountInfo = await testToken.accountInfo(account1Delegate);
  expect(delegateAccountInfo.amount.toNumber()).toBe(1);
  expect(delegateAccountInfo.originalAmount.toNumber()).toBe(2);

  await testToken.transfer(
    owner,
    account1Delegate,
    account2,
    1,
  );

  delegateAccountInfo = await testToken.accountInfo(account1Delegate);
  expect(delegateAccountInfo.amount.toNumber()).toBe(0);
  expect(delegateAccountInfo.originalAmount.toNumber()).toBe(2);

  await expect(
    testToken.transfer(
      owner,
      account1Delegate,
      account2,
      1,
    )
  ).rejects.toThrow();
});


test('set owner', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url);
  const owner = await newAccountWithTokens(connection);
  const newOwner = await newAccountWithTokens(connection);

  const account = await testToken.newAccount(owner);

  await testToken.setOwner(owner, account, newOwner.publicKey);
  await expect(
    testToken.setOwner(owner, account, newOwner.publicKey)
  ).rejects.toThrow();

  await testToken.setOwner(newOwner, account, owner.publicKey);
  await expect(
    testToken.setOwner(newOwner, account, owner.publicKey)
  ).rejects.toThrow();

});
