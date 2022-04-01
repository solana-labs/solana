import { SolanaPingProvider } from "providers/stats/SolanaPingProvider";
import React from "react";
import { SolanaClusterStatsProvider } from "./solanaClusterStats";

type Props = { children: React.ReactNode };
export function StatsProvider({ children }: Props) {
  return (
    <SolanaClusterStatsProvider>
      <SolanaPingProvider>{children}</SolanaPingProvider>
    </SolanaClusterStatsProvider>
  );
}
