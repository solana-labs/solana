// @flow

/**
 * The connection url to use when running unit tests against a live cluster
 */

export const MOCK_PORT = 9999;
export const url = process.env.TEST_LIVE
  ? 'http://localhost:8328/'
  : 'http://localhost:9999/';

//export const url = 'https://devnet.safecoin.org/';
//export const url = 'http://devnet.safecoin.org/';
