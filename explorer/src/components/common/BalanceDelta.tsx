import React from "react";
import { BigNumber } from "bignumber.js";
import { SandBalance} from "utils";

export function BalanceDelta({
  delta,
  isSand = false,
}: {
  delta: BigNumber;
  isSand?: boolean;
}) {
  let sands;

  if (isSand) {
    sands = <SandBalancelamports={delta.toNumber()} />;
  }

  if (delta.gt(0)) {
    return (
      <span className="badge bg-success-soft">
        +{isSand ? sands : delta.toString()}
      </span>
    );
  } else if (delta.lt(0)) {
    return (
      <span className="badge bg-warning-soft">
        {isSand ? <>-{sands}</> : delta.toString()}
      </span>
    );
  }

  return <span className="badge bg-secondary-soft">0</span>;
}
