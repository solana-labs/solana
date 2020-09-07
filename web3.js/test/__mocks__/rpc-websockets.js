// @flow

import {Client as LiveClient} from 'rpc-websockets';

// Define TEST_LIVE in the environment to test against the real full node
// identified by `url` instead of using the mock
export const mockRpcEnabled = !process.env.TEST_LIVE;

let mockNotice = true;

class MockClient {
  constructor(url: string) {
    if (mockNotice) {
      console.log(
        'Note: rpc-websockets mock is disabled, testing live against',
        url,
      );
      mockNotice = false;
    }
  }

  connect() {}
  close() {}
  on() {}
  call(): Promise<Object> {
    throw new Error('call unsupported');
  }
}

const Client = mockRpcEnabled ? MockClient : LiveClient;
export {Client};
