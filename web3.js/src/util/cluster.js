//@flow

/**
 * @private
 */
const endpoint = {
  http: {
    devnet: 'http://api.devnet.safecoin.org',
    testnet: 'http://api.testnet.safecoin.org',
    'mainnet-beta': 'http://api.mainnet-beta.safecoin.org',
  },
  https: {
    devnet: 'https://api.devnet.safecoin.org',
    testnet: 'https://api.testnet.safecoin.org',
    'mainnet-beta': 'https://api.mainnet-beta.safecoin.org',
  },
};

export type Cluster = 'devnet' | 'testnet' | 'mainnet-beta';

/**
 * Retrieves the RPC API URL for the specified cluster
 */
export function clusterApiUrl(cluster?: Cluster, tls?: boolean): string {
  const key = tls === false ? 'http' : 'https';

  if (!cluster) {
    return endpoint[key]['devnet'];
  }

  const url = endpoint[key][cluster];
  if (!url) {
    throw new Error(`Unknown ${key} cluster: ${cluster}`);
  }
  return url;
}
