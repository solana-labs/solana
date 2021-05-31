import React from "react";
import classNames from "classnames";
import { LoadingCard } from "./common/LoadingCard";
import { ErrorCard } from "./common/ErrorCard";
import { Sparkline, Direction } from "./Sparkline";
import { normalizePercentage } from "utils";
import {
  useCoinGeckoCategoryTokens,
  COIN_GECKO_SOLANA_CATEGORY,
  CoinGeckoToken,
} from "utils/coingecko";

export function TokenRow({
  token,
  rank,
}: {
  token: CoinGeckoToken;
  rank: number;
}) {
  return (
    <tr>
      <td>
        <span className="badge badge-soft-gray badge-pill">{rank}</span>
      </td>
      <td className="">
        <img className="token-icon mr-3" src={token.image} alt={token.name} />
        {token.name}
      </td>
      <td className="text-muted">{token.symbol.toUpperCase()}</td>
      <td className="text-right">
        {token.current_price && "$" + token.current_price}
      </td>
      <td
        className={classNames(
          "text-right",
          token.price_change_percentage_1h_in_currency > 0
            ? "change-positive"
            : "change-negative"
        )}
      >
        {normalizePercentage(token.price_change_percentage_1h_in_currency, 2)}
      </td>
      <td
        className={classNames(
          "text-right",
          token.price_change_percentage_1h_in_currency > 0
            ? "change-positive"
            : "change-negative"
        )}
      >
        {normalizePercentage(token.price_change_percentage_24h, 2)}
      </td>
      <td
        className={classNames(
          "text-right",
          token.price_change_percentage_1h_in_currency > 0
            ? "change-positive"
            : "change-negative"
        )}
      >
        {normalizePercentage(token.price_change_percentage_7d_in_currency, 2)}
      </td>
      <td className="text-right">
        {token.total_volume?.toLocaleString("en-US")}
      </td>
      <td className="text-right">
        {token.market_cap?.toLocaleString("en-US")}
      </td>
      <td>
        <Sparkline
          values={token.sparkline_in_7d.price}
          direction={
            token.price_change_percentage_7d_in_currency > 0
              ? Direction.Positive
              : Direction.Negative
          }
        />
      </td>
    </tr>
  );
}

export function TokensCard() {
  const [error, loading, tokens] = useCoinGeckoCategoryTokens(
    COIN_GECKO_SOLANA_CATEGORY
  );

  if (error) return <ErrorCard text={error.toString()} />;

  if (loading) return <LoadingCard />;

  if (tokens.length)
    return (
      <>
        <div className="card">
          <div className="card-header">
            <div className="row align-items-center">
              <div className="col">
                <h4 className="card-header-title">Tokens</h4>
              </div>
            </div>
          </div>

          <div className="table-responsive mb-0">
            <table className="table table-sm table-nowrap card-table">
              <thead>
                <tr>
                  <th className="text-muted">Rank</th>
                  <th className="text-muted">Name</th>
                  <th></th>
                  <th className="text-muted text-right">Price</th>
                  <th className="text-muted text-right">1h</th>
                  <th className="text-muted text-right">24h</th>
                  <th className="text-muted text-right">7d</th>
                  <th className="text-muted text-right">24h Volume</th>
                  <th className="text-muted text-right">Mkt Cap</th>
                  <th className="text-muted">Last 7 Days</th>
                </tr>
              </thead>
              <tbody className="list">
                {tokens.map((token, index) => (
                  <TokenRow key={index} token={token} rank={index + 1} />
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </>
    );

  return null;
}
