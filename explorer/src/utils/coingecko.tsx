import React from "react";
// @ts-ignore
import * as CoinGecko from "coingecko-api";

const PRICE_REFRESH = 10000;

export const COIN_GECKO_SOLANA_CATEGORY = "solana-ecosystem";

const CoinGeckoClient = new CoinGecko();

export enum CoingeckoStatus {
  Success,
  FetchFailed,
}

export interface CoinInfo {
  price: number;
  volume_24: number;
  market_cap: number;
  price_change_percentage_24h: number;
  market_cap_rank: number;
  last_updated: Date;
}

export interface CoinInfoResult {
  data: {
    market_data: {
      current_price: {
        usd: number;
      };
      total_volume: {
        usd: number;
      };
      market_cap: {
        usd: number;
      };
      price_change_percentage_24h: number;
      market_cap_rank: number;
    };
    last_updated: string;
  };
}

export type CoinGeckoResult = {
  coinInfo?: CoinInfo;
  status: CoingeckoStatus;
};

export function useCoinGecko(coinId?: string): CoinGeckoResult | undefined {
  const [coinInfo, setCoinInfo] = React.useState<CoinGeckoResult>();
  React.useEffect(() => {
    let interval: NodeJS.Timeout | undefined;
    if (coinId) {
      const getCoinInfo = () => {
        CoinGeckoClient.coins
          .fetch(coinId)
          .then((info: CoinInfoResult) => {
            setCoinInfo({
              coinInfo: {
                price: info.data.market_data.current_price.usd,
                volume_24: info.data.market_data.total_volume.usd,
                market_cap: info.data.market_data.market_cap.usd,
                market_cap_rank: info.data.market_data.market_cap_rank,
                price_change_percentage_24h:
                  info.data.market_data.price_change_percentage_24h,
                last_updated: new Date(info.data.last_updated),
              },
              status: CoingeckoStatus.Success,
            });
          })
          .catch((error: any) => {
            setCoinInfo({
              status: CoingeckoStatus.FetchFailed,
            });
          });
      };

      getCoinInfo();
      interval = setInterval(() => {
        getCoinInfo();
      }, PRICE_REFRESH);
    }
    return () => {
      if (interval) {
        clearInterval(interval);
      }
    };
  }, [setCoinInfo, coinId]);

  return coinInfo;
}

export type CoinGeckoTokenStats = {
  id: string;
  symbol: string;
  name: string;
  image: string;
  current_price: number;
  market_cap: number;
  market_cap_rank: number;
  fully_diluted_valuation: number;
  total_volume: number;
  high_24h: number;
  low_24h: number;
  price_change_24h: number;
  price_change_percentage_24h: number;
  market_cap_change_24h: number;
  market_cap_change_percentage_24h: number;
  circulating_supply: number;
  total_supply: number;
  max_supply: number;
  ath: number;
  ath_change_percentage: number;
  ath_date: Date;
  atl: number;
  atl_change_percentage: number;
  atl_date: Date;
  last_updated: Date;
  sparkline_in_7d: {
    price: Array<number>;
  };
  price_change_percentage_1h_in_currency: number;
  price_change_percentage_24h_in_currency: number;
  price_change_percentage_7d_in_currency: number;
};

export function useCoinGeckoCategoryTokens(
  categoryId: string
): [Error | null, Boolean, Array<CoinGeckoTokenStats>] {
  const [tokens, setTokens] = React.useState<Array<CoinGeckoTokenStats>>([]);
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState(null);

  React.useEffect(() => {
    let interval: NodeJS.Timeout | undefined;
    const fetchSolanaStats = () => {
      setError(null);
      setLoading(true);
      return CoinGeckoClient.coins
        .markets({
          category: categoryId,
          price_change_percentage: "1h,24h,7d",
          sparkline: true,
        })
        .then(({ data }: { data: Array<CoinGeckoTokenStats> }) => {
          setLoading(false);
          setTokens(data);
        })
        .catch((error: any) => {
          setLoading(false);
          setError(error);
        });
    };

    fetchSolanaStats();
    interval = setInterval(() => {
      fetchSolanaStats();
    }, PRICE_REFRESH);
    return () => {
      if (interval) {
        clearInterval(interval);
      }
    };
  }, [categoryId]);
  return [error, loading, tokens];
}

export type CoinGeckoCategoryStats = {
  id: string;
  name: string;
  market_cap: number;
  market_cap_change_24h: number;
  volume_24h: number;
  updated_at: Date;
};

export function useCoinGeckoCategoryStats(
  categoryId: string
): [Error | null, Boolean, CoinGeckoCategoryStats | undefined] {
  const [categoryStats, setCategoryStats] =
    React.useState<CoinGeckoCategoryStats>();
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState(null);

  React.useEffect(() => {
    let interval: NodeJS.Timeout | undefined;
    const fetchCategoryStats = () => {
      setError(null);
      setLoading(true);
      return fetch(`https://api.coingecko.com/api/v3/coins/categories`)
        .then((response) => response.json())
        .then((data: Array<CoinGeckoCategoryStats>) => {
          setLoading(false);

          // CoinGecko doesn't have an API (yet) that lets us
          // fetch stats for a category so we fetch all of them
          // and filter on the client
          const statsForCategory = data.find(
            (category: CoinGeckoCategoryStats) => category.id === categoryId
          );

          if (statsForCategory) {
            setCategoryStats(statsForCategory);
          } else {
            throw new Error(`Couldn't fetch stats for ${categoryId}`);
          }
        })
        .catch((error) => {
          setLoading(false);
          setError(error);
        });
    };

    fetchCategoryStats();
    interval = setInterval(() => {
      fetchCategoryStats();
    }, PRICE_REFRESH);
    return () => {
      if (interval) {
        clearInterval(interval);
      }
    };
  }, [categoryId]);
  return [error, loading, categoryStats];
}
