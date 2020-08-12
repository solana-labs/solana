import React from "react";
import { SupplyProvider } from "./supply";
import { LargestAccountsProvider } from "./largest";

type ProviderProps = { children: React.ReactNode };
export function MintsProvider({ children }: ProviderProps) {
  return (
    <SupplyProvider>
      <LargestAccountsProvider>{children}</LargestAccountsProvider>
    </SupplyProvider>
  );
}
