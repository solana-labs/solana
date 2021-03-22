import React from "react";
import { SolanaBeachProvider } from "./solanaBeach";
import { SolanaClusterStatsProvider } from "./solanaClusterStats";

type Props = { children: React.ReactNode };
export function StatsProvider({ children }: Props) {
  return (
    <SolanaClusterStatsProvider>
      <SolanaBeachProvider>{children}</SolanaBeachProvider>
    </SolanaClusterStatsProvider>
  );
}
