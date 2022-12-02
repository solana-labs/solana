import {Client as LiveClient} from 'rpc-websockets';
import {expect} from 'chai';
import {createSandbox} from 'sinon';

import {Connection} from '../../src';

type RpcRequest = {
  method: string;
  params?: Array<any>;
  subscriptionEstablishmentPromise?: Promise<void>;
};

type RpcResponse = {
  context: {
    slot: number;
  };
  value: any | Promise<any>;
};

const mockRpcSocket: Array<[RpcRequest, RpcResponse | Promise<RpcResponse>]> =
  [];
const sandbox = createSandbox();

export const mockRpcMessage = ({
  method,
  params,
  result,
  subscriptionEstablishmentPromise,
}: {
  method: string;
  params: Array<any>;
  result: any | Promise<any>;
  subscriptionEstablishmentPromise?: Promise<void>;
}) => {
  mockRpcSocket.push([
    {method, params, subscriptionEstablishmentPromise},
    {
      context: {slot: 11},
      value: result,
    },
  ]);
};

export const stubRpcWebSocket = (connection: Connection) => {
  const rpcWebSocket = connection._rpcWebSocket;
  const mockClient = new MockClient(rpcWebSocket);
  sandbox.stub(rpcWebSocket, 'connect').callsFake(() => {
    mockClient.connect();
  });
  sandbox.stub(rpcWebSocket, 'close').callsFake(() => {
    mockClient.close();
  });
  sandbox
    .stub(rpcWebSocket, 'call')
    .callsFake((method: string, params: any) => {
      return mockClient.call(method, params);
    });
};

export const restoreRpcWebSocket = (connection: Connection) => {
  connection._rpcWebSocket.close();
  if (connection._rpcWebSocketIdleTimeout !== null) {
    clearTimeout(connection._rpcWebSocketIdleTimeout);
    connection._rpcWebSocketIdleTimeout = null;
  }
  sandbox.restore();
};

function isPromise<T>(obj: PromiseLike<T> | T): obj is PromiseLike<T> {
  return (
    !!obj &&
    (typeof obj === 'object' || typeof obj === 'function') &&
    typeof (obj as any).then === 'function'
  );
}

class MockClient {
  client: LiveClient;
  mockOpen = false;
  subscriptionCounter = 0;

  constructor(rpcWebSocket: LiveClient) {
    this.client = rpcWebSocket;
  }

  connect() {
    if (!this.mockOpen) {
      this.mockOpen = true;
      this.client.emit('open');
    }
  }

  close() {
    if (this.mockOpen) {
      this.mockOpen = false;
      this.client.emit('close');
    }
  }

  async call(method: string, params: Array<any>): Promise<Object> {
    expect(mockRpcSocket.length).to.be.at.least(1);
    const [mockRequest, mockResponse] = mockRpcSocket.shift() as [
      RpcRequest,
      RpcResponse,
    ];

    expect(method).to.eq(mockRequest.method);
    if (method.endsWith('Unsubscribe')) {
      expect(params.length).to.eq(1);
      expect(params[0]).to.be.a('number');
    } else {
      expect(params).to.eql(mockRequest.params);
    }

    if (mockRequest.subscriptionEstablishmentPromise) {
      await mockRequest.subscriptionEstablishmentPromise;
    }

    let id = ++this.subscriptionCounter;
    const response = {
      subscription: id,
      result: {
        ...mockResponse,
        value: isPromise(mockResponse.value)
          ? await mockResponse.value
          : mockResponse.value,
      },
    };

    setImmediate(() => {
      const eventName = method.replace('Subscribe', 'Notification');
      this.client.emit(eventName, response);
    });

    return Promise.resolve(id);
  }
}
