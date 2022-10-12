import React from "react";
import { lamportsToSolString } from "src/utils";

export function SolBalance({
  lamports,
  maximumFractionDigits = 9,
}: {
  lamports: number | bigint;
  maximumFractionDigits?: number;
}) {
  return (
    <span>
      ◎
      <span className="font-monospace">
        {lamportsToSolString(lamports, maximumFractionDigits)}
      </span>
    </span>
  );
}
