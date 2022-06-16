import React from "react";
import { TopAccountsCard } from "src/components/TopAccountsCard";
import { SupplyCard } from "src/components/SupplyCard";

export function SupplyPage() {
  return (
    <div className="container mt-4">
      <SupplyCard />
      <TopAccountsCard />
    </div>
  );
}

export default SupplyPage;
