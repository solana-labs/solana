import React from "react";
import { LargestAccountsProvider } from "./largest";
import { TokenRegistryProvider } from "./token-registry";

type ProviderProps = { children: React.ReactNode };
export function MintsProvider({ children }: ProviderProps) {
  return (
    <TokenRegistryProvider>
      <LargestAccountsProvider>{children}</LargestAccountsProvider>
    </TokenRegistryProvider>
  );
}
