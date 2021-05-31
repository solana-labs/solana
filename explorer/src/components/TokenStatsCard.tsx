import React from "react";
// import { useSupply, useFetchSupply, Status } from "providers/supply";
import { LoadingCard } from "./common/LoadingCard";
// import { ErrorCard } from "./common/ErrorCard";
// import { lamportsToSolString } from "utils";
import { useCoinGeckoTokenStats } from "utils/coingecko";
import { TableCardBody } from "./common/TableCardBody";

export function TokenStatsCard() {
  //   const supply = useSupply();
  //   const fetchSupply = useFetchSupply();

  const tokenStats = useCoinGeckoTokenStats();

  const solanaEcosystemStats = tokenStats.find(
    ({ id }) => id === "solana-ecosystem"
  ) as any;

  //   // Fetch supply on load
  //   React.useEffect(() => {
  //     if (supply === Status.Idle) fetchSupply();
  //   }, []); // eslint-disable-line react-hooks/exhaustive-deps

  //   if (supply === Status.Disconnected) {
  //     return <ErrorCard text="Not connected to the cluster" />;
  //   }

  //   if (supply === Status.Idle || supply === Status.Connecting)
  //     return <LoadingCard />;

  //   if (typeof supply === "string") {
  //     return <ErrorCard text={supply} retry={fetchSupply} />;
  //   }

  return (
    <div className="card">
      {renderHeader()}

      {solanaEcosystemStats ? (
        <TableCardBody>
          <tr>
            <td className="w-100">Market Capitalization</td>
            <td className="text-lg-right">
              {/* {lamportsToSolString(supply.total, 0)} */}$
              {Number(
                solanaEcosystemStats?.market_cap?.toFixed(0)
              ).toLocaleString("en-US")}
            </td>
          </tr>

          <tr>
            <td className="w-100">Trading Volume</td>
            <td className="text-lg-right">
              {/* {lamportsToSolString(supply.circulating, 0)} */}$
              {Number(
                solanaEcosystemStats?.volume_24h?.toFixed(0)
              ).toLocaleString("en-US")}
            </td>
          </tr>
        </TableCardBody>
      ) : (
        <LoadingCard />
      )}
    </div>
  );
}

const renderHeader = () => {
  return (
    <div className="card-header">
      <div className="row align-items-center">
        <div className="col">
          <h4 className="card-header-title">Token Stats</h4>
        </div>
      </div>
    </div>
  );
};
