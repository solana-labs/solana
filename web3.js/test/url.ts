import {AbortController as AbortControllerPolyfill} from 'node-abort-controller';
/**
 * The connection url to use when running unit tests against a live cluster
 */
export const MOCK_PORT = 9999;

declare var process: {
  env: {
    TEST_LIVE: string;
  };
  version: string;
};

export const url = process.env.TEST_LIVE
  ? 'http://localhost:8899/'
  : 'http://localhost:9999/';

export const wsUrl = process.env.TEST_LIVE
  ? 'ws://localhost:8900/'
  : 'ws://localhost:9999/';

export const nodeVersion = Number(process.version.split('.')[0]);

export const Node14Controller = function () {
  return new AbortControllerPolyfill();
};

//export const url = 'https://api.devnet.solana.com/';
//export const url = 'http://api.devnet.solana.com/';
