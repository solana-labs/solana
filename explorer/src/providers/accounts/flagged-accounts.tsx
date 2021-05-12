import React from "react";
import { fetch } from "cross-fetch";

const FLAGGED_REGISTRY =
  "https://solana-labs.github.io/solana-flagged-accounts/flagged.txt";

type FlaggedMap = Map<string, boolean>;
type ProviderProps = { children: React.ReactNode };

const FlaggedContext = React.createContext<FlaggedMap>(new Map());

export function FlaggedAccountsProvider({ children }: ProviderProps) {
  const [flaggedAccounts, setFlaggedAccounts] = React.useState<FlaggedMap>(
    new Map()
  );

  React.useEffect(() => {
    fetch(FLAGGED_REGISTRY)
      .then((res) => {
        return res.text();
      })
      .then((body: string) => {
        const flaggedAccounts = new Map<string, boolean>();
        body
          .split("\n")
          .forEach((account) => flaggedAccounts.set(account, true));
        setFlaggedAccounts(flaggedAccounts);
      });
  }, []);

  return (
    <FlaggedContext.Provider value={flaggedAccounts}>
      {children}
    </FlaggedContext.Provider>
  );
}

export function useFlaggedAccounts() {
  const flaggedAccounts = React.useContext(FlaggedContext);
  if (!flaggedAccounts) {
    throw new Error(
      `useFlaggedAccounts must be used within a AccountsProvider`
    );
  }

  return { flaggedAccounts };
}
