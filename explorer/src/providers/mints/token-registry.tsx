import React from "react";
import {
  TokenListProvider,
  KnownToken,
  KnownTokenMap,
} from "@solana/spl-token-registry";
import { clusterSlug, useCluster } from "providers/cluster";

const TokenRegistryContext = React.createContext<KnownTokenMap>(new Map());

type ProviderProps = { children: React.ReactNode };

export function TokenRegistryProvider({ children }: ProviderProps) {
  const [tokenRegistry, setTokenRegistry] = React.useState<KnownTokenMap>(
    new Map()
  );
  const { cluster } = useCluster();

  React.useEffect(() => {
    new TokenListProvider()
      .resolve(clusterSlug(cluster))
      .then((tokens: KnownToken[]) => {
        setTokenRegistry(
          tokens.reduce((map: KnownTokenMap, item: KnownToken) => {
            if (item.tokenName && item.tokenSymbol) {
              map.set(item.mintAddress, item);
            }
            return map;
          }, new Map())
        );
      });
  }, [cluster]);

  return (
    <TokenRegistryContext.Provider value={tokenRegistry}>
      {children}
    </TokenRegistryContext.Provider>
  );
}

export function useTokenRegistry() {
  const tokenRegistry = React.useContext(TokenRegistryContext);

  if (!tokenRegistry) {
    throw new Error(`useTokenRegistry must be used within a MintsProvider`);
  }

  return { tokenRegistry };
}
