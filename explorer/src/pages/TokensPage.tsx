import React from "react";
import { TokenStatsCard } from "components/TokenStatsCard";
import { TokensCard } from "components/TokensCard";

export function TokensPage() {
  return (
    <div className="container mt-4">
      <TokenStatsCard />
      <TokensCard />
    </div>
  );
}
