import {expect} from 'chai';

import {makeWebsocketUrl} from '../src/utils/makeWebsocketUrl';

const INVALID_URLS = [
  '',
  '0.0.0.0',
  'localhost',
  'www.no-protocol.com',
  '//api.protocol.relative.com',
];
const TEST_CASES = [
  // Non-https => `ws`
  ['http://api.devnet.solana.com/', 'ws://api.devnet.solana.com/'],
  ['gopher://gopher.example.com/', 'ws://gopher.example.com/'],
  ['http://localhost/', 'ws://localhost/'],
  // `https` => `wss`
  ['https://api.devnet.solana.com/', 'wss://api.devnet.solana.com/'],
  // IPv4 address
  ['https://192.168.0.1/', 'wss://192.168.0.1/'],
  // IPv6 address
  ['https://[0:0:0:0:0:0:0:0]/', 'wss://[0:0:0:0:0:0:0:0]/'],
  ['https://[::]/', 'wss://[::]/'],
  ['https://[::1]/', 'wss://[::1]/'],
  // Increment port if supplied
  ['https://api.devnet.solana.com:80/', 'wss://api.devnet.solana.com:81/'],
  ['https://192.168.0.1:443/', 'wss://192.168.0.1:444/'],
  ['https://[::]:8080/', 'wss://[::]:8081/'],
  // No trailing slash
  ['http://api.devnet.solana.com', 'ws://api.devnet.solana.com'],
  ['https://api.devnet.solana.com', 'wss://api.devnet.solana.com'],
  ['https://api.devnet.solana.com:80', 'wss://api.devnet.solana.com:81'],
  // Username
  ['https://alice@private.com', 'wss://alice@private.com'],
  // Username/password
  ['https://bob:password@private.com', 'wss://bob:password@private.com'],
];

describe('makeWebsocketUrl', () => {
  TEST_CASES.forEach(([inputUrl, outputUrl]) => {
    it(`converts \`${inputUrl}\` to \`${outputUrl}\``, () => {
      expect(makeWebsocketUrl(inputUrl)).to.equal(outputUrl);
    });
  });
  INVALID_URLS.forEach(invalidUrl => {
    it(`fatals when called with invalid url \`${invalidUrl}\``, () => {
      expect(() => {
        makeWebsocketUrl(invalidUrl);
      }).to.throw();
    });
  });
});
