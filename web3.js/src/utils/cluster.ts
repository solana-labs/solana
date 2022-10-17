const endpoint = {
  http: {
    devnet: 'http://api.devnet.solana.com',
    testnet: 'http://api.testnet.solana.com',
    'mainnet-beta': 'http://api.mainnet-beta.solana.com',
    localnet: 'http://localhost:8899',
  },
  https: {
    devnet: 'https://api.devnet.solana.com',
    testnet: 'https://api.testnet.solana.com',
    'mainnet-beta': 'https://api.mainnet-beta.solana.com',
    localnet: 'https://localhost:8899',
  },
};

export type Cluster = 'devnet' | 'testnet' | 'mainnet-beta' | 'localnet';

/**
 * Retrieves the RPC API URL for the specified cluster
 */
export function clusterApiUrl(cluster?: Cluster, tls?: boolean): string {
  const key =
    tls === false || (tls == undefined && cluster === 'localnet')
      ? 'http'
      : 'https';

  if (!cluster) {
    return endpoint[key]['devnet'];
  }

  const url = endpoint[key][cluster];
  if (!url) {
    throw new Error(`Unknown ${key} cluster: ${cluster}`);
  }
  return url;
}
