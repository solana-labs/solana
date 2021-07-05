import React from "react";
import {
  TokenListProvider,
  TokenInfoMap,
  TokenInfo,
  TokenListContainer,
} from "@solana/spl-token-registry";
import { clusterSlug, useCluster } from "providers/cluster";

const TokenRegistryContext = React.createContext<TokenInfoMap>(new Map());

type ProviderProps = { children: React.ReactNode };

export function TokenRegistryProvider({ children }: ProviderProps) {
  const [tokenRegistry, setTokenRegistry] = React.useState<TokenInfoMap>(
    new Map()
  );
  const { cluster } = useCluster();

  React.useEffect(() => {
    new TokenListProvider().resolve().then((tokens: TokenListContainer) => {
      const tokenList = tokens
        .filterByClusterSlug(clusterSlug(cluster))
        .getList();

      setTokenRegistry(
        tokenList.reduce((map: TokenInfoMap, item: TokenInfo) => {
          map.set(item.address, item);
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
