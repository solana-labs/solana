import React from "react";
import { TopAccountsCard } from "components/TopAccountsCard";
import { SupplyCard } from "components/SupplyCard";
import { Cluster, useCluster } from "providers/cluster";

export function SupplyPage() {
  const cluster = useCluster();
  return (
    <div className="container mt-4">
      <SupplyCard />
      {cluster.cluster === Cluster.Custom ? <TopAccountsCard /> : null}
    </div>
  );
}
