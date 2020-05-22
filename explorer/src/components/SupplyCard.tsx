import React from "react";
import { useSupply, useFetchSupply } from "providers/supply";
import LoadingCard from "./common/LoadingCard";
import ErrorCard from "./common/ErrorCard";
import { lamportsToSolString } from "utils";
import TableCardBody from "./common/TableCardBody";

export default function SupplyCard() {
  const supply = useSupply();
  const fetchSupply = useFetchSupply();

  if (typeof supply === "boolean") {
    if (supply) return <LoadingCard />;
    return <ErrorCard text="Not connected to the cluster" />;
  }

  if (typeof supply === "string") {
    return <ErrorCard text={supply} retry={fetchSupply} />;
  }

  return (
    <div className="card">
      {renderHeader()}

      <TableCardBody>
        <tr>
          <td className="w-100">Total Supply (SOL)</td>
          <td>{lamportsToSolString(supply.total)}</td>
        </tr>

        <tr>
          <td className="w-100">Circulating Supply (SOL)</td>
          <td>{lamportsToSolString(supply.circulating)}</td>
        </tr>
      </TableCardBody>
    </div>
  );
}

const renderHeader = () => {
  return (
    <div className="card-header">
      <div className="row align-items-center">
        <div className="col">
          <h4 className="card-header-title">Supply Stats</h4>
        </div>
      </div>
    </div>
  );
};
