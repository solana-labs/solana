import React from "react";
import { SolanaClusterStatsProvider } from "./solanaClusterStats";

type Props = { children: React.ReactNode };
export function StatsProvider({ children }: Props) {
  return <SolanaClusterStatsProvider>{children}</SolanaClusterStatsProvider>;
}
