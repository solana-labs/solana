import React from "react";
import { SolanaBeachProvider } from "./solanaBeach";

type Props = { children: React.ReactNode };
export function StatsProvider({ children }: Props) {
  return <SolanaBeachProvider>{children}</SolanaBeachProvider>;
}
