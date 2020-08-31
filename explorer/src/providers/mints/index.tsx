import React from "react";
import { LargestAccountsProvider } from "./largest";

type ProviderProps = { children: React.ReactNode };
export function MintsProvider({ children }: ProviderProps) {
  return <LargestAccountsProvider>{children}</LargestAccountsProvider>;
}
