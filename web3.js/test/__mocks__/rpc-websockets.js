// @flow

import {Client as LiveClient} from 'rpc-websockets';
import EventEmitter from 'events';

type RpcRequest = {
  method: string,
  params?: Array<any>,
};

type RpcResponse = {
  context: {
    slot: number,
  },
  value: any,
};

// Define TEST_LIVE in the environment to test against the real full node
// identified by `url` instead of using the mock
export const mockRpcEnabled = !process.env.TEST_LIVE;

export const mockRpcSocket: Array<[RpcRequest, RpcResponse]> = [];

class MockClient extends EventEmitter {
  mockOpen = false;
  subscriptionCounter = 0;

  constructor() {
    super();
  }

  connect() {
    if (!this.mockOpen) {
      this.mockOpen = true;
      this.emit('open');
    }
  }

  close() {
    if (this.mockOpen) {
      this.mockOpen = false;
      this.emit('close');
    }
  }

  notify(): Promise<any> {
    return Promise.resolve();
  }

  on(event: string, callback: Function): this {
    return super.on(event, callback);
  }

  call(method: string, params: Array<any>): Promise<Object> {
    expect(mockRpcSocket.length).toBeGreaterThanOrEqual(1);
    const [mockRequest, mockResponse] = mockRpcSocket.shift();

    expect(method).toBe(mockRequest.method);
    expect(params).toMatchObject(mockRequest.params);

    let id = this.subscriptionCounter++;
    const response = {
      subscription: id,
      result: mockResponse,
    };

    setImmediate(() => {
      const eventName = method.replace('Subscribe', 'Notification');
      this.emit(eventName, response);
    });

    return Promise.resolve(id);
  }
}

const Client = mockRpcEnabled ? MockClient : LiveClient;
export {Client};
