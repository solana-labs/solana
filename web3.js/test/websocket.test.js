// @flow
import bs58 from 'bs58';

import {Connection} from '../src';
import {url} from './url';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {sleep} from '../src/util/sleep';

describe('websocket', () => {
  if (mockRpcEnabled) {
    test('no-op', () => {});
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url);

  test('connect and disconnect', async () => {
    const testSignature = bs58.encode(Buffer.alloc(64));
    const id = connection.onSignature(testSignature, () => {});

    // wait for websocket to connect
    await sleep(100);
    expect(connection._rpcWebSocketConnected).toBe(true);
    expect(connection._rpcWebSocketHeartbeat).not.toBe(null);

    // test if socket is open
    await connection._rpcWebSocket.notify('ping');

    await connection.removeSignatureListener(id);
    expect(connection._rpcWebSocketConnected).toBe(false);
    expect(connection._rpcWebSocketHeartbeat).not.toBe(null);
    expect(connection._rpcWebSocketIdleTimeout).not.toBe(null);

    // wait for websocket to disconnect
    await sleep(1100);
    expect(connection._rpcWebSocketConnected).toBe(false);
    expect(connection._rpcWebSocketHeartbeat).toBe(null);
    expect(connection._rpcWebSocketIdleTimeout).toBe(null);

    // test if socket is closed
    await expect(connection._rpcWebSocket.notify('ping')).rejects.toThrow(
      'socket not ready',
    );
  });

  test('idle timeout', async () => {
    const testSignature = bs58.encode(Buffer.alloc(64));
    const id = connection.onSignature(testSignature, () => {});

    // wait for websocket to connect
    await sleep(100);
    expect(connection._rpcWebSocketIdleTimeout).toBe(null);

    await connection.removeSignatureListener(id);
    expect(connection._rpcWebSocketIdleTimeout).not.toBe(null);

    const nextId = connection.onSignature(testSignature, () => {});
    expect(connection._rpcWebSocketIdleTimeout).toBe(null);

    await connection.removeSignatureListener(nextId);
    expect(connection._rpcWebSocketIdleTimeout).not.toBe(null);

    // wait for websocket to disconnect
    await sleep(1100);
    expect(connection._rpcWebSocketIdleTimeout).toBe(null);
  });
});
