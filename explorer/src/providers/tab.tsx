import React from "react";
import { useLocation } from "react-router-dom";

import { ACCOUNT_PATHS } from "./accounts";

export type Tab = "Transactions" | "Accounts";
const StateContext = React.createContext<Tab | undefined>(undefined);

type TabProviderProps = { children: React.ReactNode };
export function TabProvider({ children }: TabProviderProps) {
  const location = useLocation();
  const paths = location.pathname.slice(1).split("/");
  let tab: Tab = "Transactions";
  if (ACCOUNT_PATHS.includes(paths[0].toLowerCase())) {
    tab = "Accounts";
  }

  return <StateContext.Provider value={tab}>{children}</StateContext.Provider>;
}

export function useCurrentTab() {
  const context = React.useContext(StateContext);
  if (!context) {
    throw new Error(`useCurrentTab must be used within a TabProvider`);
  }
  return context;
}
