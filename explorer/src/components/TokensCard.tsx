import React from "react";
// import { Link } from "react-router-dom";
import { Line } from "react-chartjs-2";
import { LoadingCard } from "./common/LoadingCard";
// import { Location } from "history";
// import { AccountBalancePair } from "@solana/web3.js";
// import { useRichList, useFetchRichList, Status } from "providers/richList";
// import { LoadingCard } from "./common/LoadingCard";
// import { ErrorCard } from "./common/ErrorCard";
// import { lamportsToSolString } from "utils";
// import { useQuery } from "utils/url";
// import { useSupply } from "providers/supply";
import { useCoinGeckoTokens } from "utils/coingecko";
// import { normalizeTokenAmount } from "utils";
// import { Address } from "./common/Address";

export function TokensCard() {
  const tokens = useCoinGeckoTokens();

  //   if (richList === Status.Disconnected) {
  //     return <ErrorCard text="Not connected to the cluster" />;
  //   }

  //   if (richList === Status.Connecting) {
  //     return <LoadingCard />;
  //   }

  //   if (typeof richList === "string") {
  //     return <ErrorCard text={richList} retry={fetchRichList} />;
  //   }

  //   let supplyCount: number;
  //   let accounts, header;

  //   if (richList !== Status.Idle) {
  //     switch (filter) {
  //       case "nonCirculating": {
  //         accounts = richList.nonCirculating;
  //         supplyCount = supply.nonCirculating;
  //         header = "Non-Circulating";
  //         break;
  //       }
  //       case "all": {
  //         accounts = richList.total;
  //         supplyCount = supply.total;
  //         header = "Total";
  //         break;
  //       }
  //       case "circulating":
  //       default: {
  //         accounts = richList.circulating;
  //         supplyCount = supply.circulating;
  //         header = "Circulating";
  //         break;
  //       }
  //     }
  //   }

  //   const chartOptions = {
  //     responsive: false,
  //     legend: {
  //       display: false,
  //     },
  //     elements: {
  //       line: {
  //         borderColor: "#19be56",
  //         borderWidth: 1,
  //       },
  //       point: {
  //         radius: 0,
  //       },
  //     },
  //     tooltips: {
  //       enabled: false,
  //     },
  //     scales: {
  //       yAxes: [
  //         {
  //           display: false,
  //         },
  //       ],
  //       xAxes: [
  //         {
  //           display: false,
  //         },
  //       ],
  //     },
  //     maintainAspectRatio: false,
  //   };

  //   const chartData = {
  //     labels: [
  //       "Jan",
  //       "Feb",
  //       "Mar",
  //       "Apr",
  //       "May",
  //       "Jun",
  //       "Jul",
  //       "Aug",
  //       "Sep",
  //       "Oct",
  //       "Nov",
  //       "Dec",
  //     ],
  //     datasets: [
  //       {
  //         data: [435, 321, 532, 801, 1231, 1098, 732, 321, 451, 482, 513, 397],
  //       },
  //     ],
  //   };

  return (
    <>
      {/* {showDropdown && (
        <div className="dropdown-exit" onClick={() => setDropdown(false)} />
      )} */}
      {tokens.length ? (
        <div className="card">
          <div className="card-header">
            <div className="row align-items-center">
              <div className="col">
                <h4 className="card-header-title">Tokens</h4>
              </div>

              <div className="col-auto">
                {/* <FilterDropdown
                filter={filter}
                toggle={() => setDropdown((show) => !show)}
                show={showDropdown}
              /> */}
              </div>
            </div>
          </div>

          {/* {richList === Status.Idle && (
          <div className="card-body">
            <span
              className="btn btn-white ml-3 d-none d-md-inline"
              onClick={fetchRichList}
            >
              Load Largest Accounts
            </span>
          </div>
        )} */}

          {/* {accounts && ( */}
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
                {tokens &&
                  tokens.map((token: any, index) => {
                    return (
                      <tr key={index}>
                        <td>
                          <span className="badge badge-soft-gray badge-pill">
                            {index + 1}
                          </span>
                        </td>
                        <td className="">
                          <img
                            className="token-icon mr-3"
                            src={token.image}
                            alt=""
                          />
                          {token.name}
                        </td>
                        <td className="text-muted">
                          {token.symbol.toUpperCase()}
                        </td>
                        <td className="text-right">
                          {token.current_price ? `$${token.current_price}` : ""}
                        </td>
                        <td
                          className={`text-right ${
                            token.price_change_percentage_1h_in_currency > 0
                              ? "change-positive"
                              : "change-negative"
                          }`}
                        >
                          {token.price_change_percentage_1h_in_currency
                            ? `${token.price_change_percentage_1h_in_currency.toFixed(
                                2
                              )}%`
                            : ""}
                        </td>
                        <td
                          className={`text-right ${
                            token.price_change_percentage_24h > 0
                              ? "change-positive"
                              : "change-negative"
                          }`}
                        >
                          {token.price_change_percentage_24h
                            ? `${parseInt(
                                token.price_change_percentage_24h
                              ).toFixed(2)}%`
                            : ""}
                        </td>
                        <td
                          className={`text-right ${
                            token.price_change_percentage_7d_in_currency > 0
                              ? "change-positive"
                              : "change-negative"
                          }`}
                        >
                          {token.price_change_percentage_7d_in_currency
                            ? `${parseInt(
                                token.price_change_percentage_7d_in_currency
                              ).toFixed(2)}%`
                            : ""}
                        </td>
                        <td className="text-right">
                          {token.total_volume?.toLocaleString("en-US", {
                            //   minimumFractionDigits: 2,
                          })}
                        </td>
                        <td className="text-right">
                          {token.market_cap?.toLocaleString("en-US")}
                        </td>
                        <td className="">
                          <Line
                            width={75}
                            height={35}
                            data={{
                              labels: token.sparkline_in_7d.price,
                              datasets: [
                                {
                                  data: token.sparkline_in_7d.price,
                                },
                              ],
                            }}
                            options={{
                              responsive: false,
                              legend: {
                                display: false,
                              },
                              elements: {
                                line: {
                                  borderColor:
                                    token.price_change_percentage_7d_in_currency >
                                    0
                                      ? "#26e97e"
                                      : "#fa62fc",
                                  borderWidth: 1,
                                },
                                point: {
                                  radius: 0,
                                },
                              },
                              tooltips: {
                                enabled: false,
                              },
                              scales: {
                                yAxes: [
                                  {
                                    display: false,
                                  },
                                ],
                                xAxes: [
                                  {
                                    display: false,
                                  },
                                ],
                              },
                              maintainAspectRatio: false,
                            }}
                          />
                        </td>
                      </tr>
                    );
                  })}
              </tbody>
            </table>
          </div>
        </div>
      ) : (
        <LoadingCard />
      )}
    </>
  );
}
