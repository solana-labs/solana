import React from "react";

const initialState = new Map();
const FlaggedContext = React.createContext<Map<string, boolean>>(initialState);

type ProviderProps = { children: React.ReactNode };

export function FlaggedAccountsProvider({ children }: ProviderProps) {
  const [flaggedAccounts, setFlaggedAccounts] = React.useState<
    Map<string, boolean>
  >(initialState);

  React.useEffect(() => {
    window
      .fetch(
        "https://solana-labs.github.io/solana-flagged-accounts/flagged.txt"
      )
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
