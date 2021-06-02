import React from "react";
import { LoadingCard } from "./common/LoadingCard";
import { ErrorCard } from "./common/ErrorCard";
import {
  useCoinGeckoCategoryStats,
  COIN_GECKO_SOLANA_CATEGORY,
} from "utils/coingecko";

import { formatDollarValue } from "utils";
import { displayTimestampWithoutDate } from "utils/date";

export function TokenStatsCard() {
  const [error, loading, categoryStats] = useCoinGeckoCategoryStats(
    COIN_GECKO_SOLANA_CATEGORY
  );

  if (error) return <ErrorCard text={error.toString()} />;

  if (loading) return <LoadingCard />;

  if (categoryStats)
    return (
      <div className="card staking-card">
        <div className="card-body">
          <div className="d-flex flex-md-row flex-column">
            <div className="p-2 flex-fill">
              <h4>Market Capitalization</h4>
              <h1>
                <em>{formatDollarValue(categoryStats.market_cap)}</em>
              </h1>
            </div>
            <hr className="hidden-sm-up" />
            <div className="p-2 flex-fill">
              <h4>Trading Volume</h4>
              <h1>
                <em>{formatDollarValue(categoryStats.volume_24h)}</em>
              </h1>
            </div>
          </div>
          <p className="updated-time text-muted mb-0">
            Updated at{" "}
            {displayTimestampWithoutDate(categoryStats.updated_at.getTime())}
          </p>
        </div>
      </div>
    );

  return null;
}
