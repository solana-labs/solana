import React from "react";
import { SignatureResult } from "@solana/web3.js";

type InstructionProps = {
  title: string;
  children: React.ReactNode;
  result: SignatureResult;
  index: number;
};

export function InstructionCard({
  title,
  children,
  result,
  index
}: InstructionProps) {
  const [resultClass, errorString] = ixResult(result, index);
  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          <span className={`badge badge-soft-${resultClass} mr-2`}>
            #{index + 1}
          </span>
          {title}
        </h3>
        <h3 className="mb-0">
          <span className="badge badge-soft-warning text-monospace">
            {errorString}
          </span>
        </h3>
      </div>
      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <tbody className="list">{children}</tbody>
        </table>
      </div>
    </div>
  );
}

function ixResult(result: SignatureResult, index: number) {
  if (result.err) {
    const err = result.err as any;
    const ixError = err["InstructionError"];
    if (ixError && Array.isArray(ixError)) {
      const [errorIndex, error] = ixError;
      if (Number.isInteger(errorIndex) && errorIndex === index) {
        return ["warning", `Error: ${JSON.stringify(error)}`];
      }
    }
    return ["dark"];
  }
  return ["success"];
}
