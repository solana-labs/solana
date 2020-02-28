// @flow

import {Client as RpcWebSocketClient} from 'rpc-websockets';

// Define TEST_LIVE in the environment to test against the real full node
// identified by `url` instead of using the mock
export const mockRpcEnabled = !process.env.TEST_LIVE;

let mockNotice = true;

export class Client {
  client: RpcWebSocketClient;

  constructor(url, options) {
    //console.log('MockClient', url, options);
    if (!mockRpcEnabled) {
      if (mockNotice) {
        console.log(
          'Note: rpc-websockets mock is disabled, testing live against',
          url,
        );
        mockNotice = false;
      }
      this.client = new RpcWebSocketClient(url, options);
    }
  }

  connect() {
    if (!mockRpcEnabled) {
      return this.client.connect();
    }
  }

  close() {
    if (!mockRpcEnabled) {
      return this.client.close();
    }
  }

  on(event: string, callback: Function) {
    if (!mockRpcEnabled) {
      return this.client.on(event, callback);
    }
    //console.log('on', event);
  }

  async call(method: string, params: Object): Promise<Object> {
    if (!mockRpcEnabled) {
      return await this.client.call(method, params);
    }
    throw new Error('call unsupported');
  }
}
