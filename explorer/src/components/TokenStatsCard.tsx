import React from "react";
import { LoadingCard } from "./common/LoadingCard";
import { ErrorCard } from "./common/ErrorCard";
import {
  useCoinGeckoCategoryStats,
  COIN_GECKO_SOLANA_CATEGORY,
} from "utils/coingecko";

import { TableCardBody } from "./common/TableCardBody";

const formatDollarValue = (value: number, decimals: number): string =>
  "$" + Number(value.toFixed(decimals)).toLocaleString("en-US");

export function TokenStatsCard() {
  const [error, loading, categoryStats] = useCoinGeckoCategoryStats(
    COIN_GECKO_SOLANA_CATEGORY
  );

  if (error) return <ErrorCard text={error.toString()} />;

  if (loading) return <LoadingCard />;

  if (categoryStats)
    return (
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h4 className="card-header-title">Token Stats</h4>
            </div>
          </div>
        </div>
        <TableCardBody>
          <tr>
            <td className="w-100">Market Capitalization</td>
            <td className="text-lg-right">
              {formatDollarValue(categoryStats.market_cap, 0)}
            </td>
          </tr>

          <tr>
            <td className="w-100">Trading Volume</td>
            <td className="text-lg-right">
              {formatDollarValue(categoryStats.volume_24h, 0)}
            </td>
          </tr>
        </TableCardBody>
      </div>
    );

  return null;
}
