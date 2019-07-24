//@flow

import {testnetDefaultChannel} from '../../package.json';

/**
 * @private
 */
const endpoint = {
  edge: 'https://edge.testnet.solana.com:8443',
  beta: 'https://beta.testnet.solana.com:8443',
  stable: 'https://testnet.solana.com:8443',
};

/**
 * Retrieves the RPC endpoint URL for the specified testnet release
 * channel
 */
export function testnetChannelEndpoint(channel?: string): string {
  if (!channel) {
    return endpoint[testnetDefaultChannel];
  }

  if (endpoint[channel]) {
    return endpoint[channel];
  }
  throw new Error(`Unknown channel: ${channel}`);
}
