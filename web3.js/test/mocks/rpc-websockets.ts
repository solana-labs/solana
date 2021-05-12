import {Client as LiveClient} from 'rpc-websockets';
import {expect} from 'chai';
import sinon from 'sinon';

import {Connection} from '../../src';

type RpcRequest = {
  method: string;
  params?: Array<any>;
};

type RpcResponse = {
  context: {
    slot: number;
  };
  value: any;
};

const mockRpcSocket: Array<[RpcRequest, RpcResponse]> = [];
const sandbox = sinon.createSandbox();

export const mockRpcMessage = ({
  method,
  params,
  result,
}: {
  method: string;
  params: Array<any>;
  result: any;
}) => {
  mockRpcSocket.push([
    {method, params},
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

  call(method: string, params: Array<any>): Promise<Object> {
    expect(mockRpcSocket.length).to.be.at.least(1);
    const [mockRequest, mockResponse] = mockRpcSocket.shift() as [
      RpcRequest,
      RpcResponse,
    ];

    expect(method).to.eq(mockRequest.method);
    expect(params).to.eql(mockRequest.params);

    let id = ++this.subscriptionCounter;
    const response = {
      subscription: id,
      result: mockResponse,
    };

    setImmediate(() => {
      const eventName = method.replace('Subscribe', 'Notification');
      this.client.emit(eventName, response);
    });

    return Promise.resolve(id);
  }
}
