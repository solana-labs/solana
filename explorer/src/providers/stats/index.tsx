import React from "react";
import { SafecoinClusterStatsProvider } from "./solanaClusterStats";

type Props = { children: React.ReactNode };
export function StatsProvider({ children }: Props) {
  return <SafecoinClusterStatsProvider>{children}</SafecoinClusterStatsProvider>;
}
