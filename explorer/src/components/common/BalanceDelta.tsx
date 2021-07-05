import React from "react";
import { BigNumber } from "bignumber.js";
import { lamportsToSafeString } from "utils";

export function BalanceDelta({
  delta,
  isSafe = false,
}: {
  delta: BigNumber;
  isSafe?: boolean;
}) {
  let sols;

  if (isSafe) {
    sols = lamportsToSafeString(delta.toNumber());
  }

  if (delta.gt(0)) {
    return (
      <span className="badge badge-soft-success">
        +{isSafe ? sols : delta.toString()}
      </span>
    );
  } else if (delta.lt(0)) {
    return (
      <span className="badge badge-soft-warning">
        {isSafe ? <>-{sols}</> : delta.toString()}
      </span>
    );
  }

  return <span className="badge badge-soft-secondary">0</span>;
}
