//@flow

import {testnetDefaultChannel} from '../../package.json';

/**
 * @private
 */
const endpoint = {
  http: {
    edge: 'http://edge.devnet.solana.com:8899',
    beta: 'http://beta.devnet.solana.com:8899',
    stable: 'http://devnet.solana.com:8899',
  },
  https: {
    edge: 'https://edge.devnet.solana.com:8443',
    beta: 'https://beta.devnet.solana.com:8443',
    stable: 'https://devnet.solana.com:8443',
  },
};

/**
 * Retrieves the RPC endpoint URL for the specified testnet release
 * channel
 */
export function testnetChannelEndpoint(
  channel?: string,
  tls?: boolean,
): string {
  const key = tls === false ? 'http' : 'https';

  if (!channel) {
    return endpoint[key][testnetDefaultChannel];
  }

  const url = endpoint[key][channel];
  if (!url) {
    throw new Error(`Unknown ${key} channel: ${channel}`);
  }
  return url;
}
