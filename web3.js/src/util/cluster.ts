//@flow

/**
 * @private
 */
const endpoint = {
  http: {
    devnet: 'http://devnet.solana.com',
    testnet: 'http://testnet.solana.com',
    'mainnet-beta': 'http://api.mainnet-beta.solana.com',
  },
  https: {
    devnet: 'https://devnet.solana.com',
    testnet: 'https://testnet.solana.com',
    'mainnet-beta': 'https://api.mainnet-beta.solana.com',
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
