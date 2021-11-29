import React from "react";
import { Bar } from "react-chartjs-2";
import CountUp from "react-countup";
import {
  usePerformanceInfo,
  PERF_UPDATE_SEC,
  ClusterStatsStatus,
} from "providers/stats/solanaClusterStats";
import classNames from "classnames";
import { TableCardBody } from "components/common/TableCardBody";
import { ChartOptions, ChartTooltipModel } from "chart.js";
import { PerformanceInfo } from "providers/stats/solanaPerformanceInfo";
import { StatsNotReady } from "pages/ClusterStatsPage";

export function TpsCard() {
  return (
    <div className="card">
      <div className="card-header">
        <h4 className="card-header-title">Live Transaction Stats</h4>
      </div>
      <TpsCardBody />
    </div>
  );
}

function TpsCardBody() {
  const performanceInfo = usePerformanceInfo();

  if (performanceInfo.status !== ClusterStatsStatus.Ready) {
    return (
      <StatsNotReady
        error={performanceInfo.status === ClusterStatsStatus.Error}
      />
    );
  }

  return <TpsBarChart performanceInfo={performanceInfo} />;
}

type Series = "short" | "medium" | "long";
const SERIES: Series[] = ["short", "medium", "long"];
const SERIES_INFO = {
  short: {
    label: (index: number) => index,
    interval: "30m",
  },
  medium: {
    label: (index: number) => index * 4,
    interval: "2h",
  },
  long: {
    label: (index: number) => index * 12,
    interval: "6h",
  },
};

const CUSTOM_TOOLTIP = function (this: any, tooltipModel: ChartTooltipModel) {
  // Tooltip Element
  let tooltipEl = document.getElementById("chartjs-tooltip");

  // Create element on first render
  if (!tooltipEl) {
    tooltipEl = document.createElement("div");
    tooltipEl.id = "chartjs-tooltip";
    tooltipEl.innerHTML = `<div class="content"></div>`;
    document.body.appendChild(tooltipEl);
  }

  // Hide if no tooltip
  if (tooltipModel.opacity === 0) {
    tooltipEl.style.opacity = "0";
    return;
  }

  // Set Text
  if (tooltipModel.body) {
    const { label, value } = tooltipModel.dataPoints[0];
    const tooltipContent = tooltipEl.querySelector("div");
    if (tooltipContent) {
      let innerHtml = `<div class="value">${value} TPS</div>`;
      innerHtml += `<div class="label">${label}</div>`;
      tooltipContent.innerHTML = innerHtml;
    }
  }

  // Enable tooltip and set position
  const canvas: Element = this._chart.canvas;
  const position = canvas.getBoundingClientRect();
  tooltipEl.style.opacity = "1";
  tooltipEl.style.left =
    position.left + window.pageXOffset + tooltipModel.caretX + "px";
  tooltipEl.style.top =
    position.top + window.pageYOffset + tooltipModel.caretY + "px";
};

const CHART_OPTIONS = (historyMaxTps: number): ChartOptions => {
  return {
    tooltips: {
      intersect: false, // Show tooltip when cursor in between bars
      enabled: false, // Hide default tooltip
      custom: CUSTOM_TOOLTIP,
    },
    legend: {
      display: false,
    },
    scales: {
      xAxes: [
        {
          ticks: {
            display: false,
          },
          gridLines: {
            display: false,
          },
        },
      ],
      yAxes: [
        {
          ticks: {
            stepSize: 100,
            fontSize: 10,
            fontColor: "#EEE",
            beginAtZero: true,
            display: true,
            suggestedMax: historyMaxTps,
          },
          gridLines: {
            display: false,
          },
        },
      ],
    },
    animation: {
      duration: 0, // general animation time
    },
    hover: {
      animationDuration: 0, // duration of animations when hovering an item
    },
    responsiveAnimationDuration: 0, // animation duration after a resize
  };
};

type TpsBarChartProps = { performanceInfo: PerformanceInfo };
function TpsBarChart({ performanceInfo }: TpsBarChartProps) {
  const { perfHistory, avgTps, historyMaxTps } = performanceInfo;
  const [series, setSeries] = React.useState<Series>("short");
  const averageTps = Math.round(avgTps).toLocaleString("en-US");
  const transactionCount = <AnimatedTransactionCount info={performanceInfo} />;
  const seriesData = perfHistory[series];
  const chartOptions = React.useMemo(
    () => CHART_OPTIONS(historyMaxTps),
    [historyMaxTps]
  );

  const seriesLength = seriesData.length;
  const chartData: Chart.ChartData = {
    labels: seriesData.map((val, i) => {
      return `${SERIES_INFO[series].label(seriesLength - i)}min ago`;
    }),
    datasets: [
      {
        backgroundColor: "#00D192",
        hoverBackgroundColor: "#00D192",
        borderWidth: 0,
        data: seriesData.map((val) => val || 0),
      },
    ],
  };

  return (
    <>
      <TableCardBody>
        <tr>
          <td className="w-100">Transaction count</td>
          <td className="text-lg-end font-monospace">{transactionCount} </td>
        </tr>
        <tr>
          <td className="w-100">Transactions per second (TPS)</td>
          <td className="text-lg-end font-monospace">{averageTps} </td>
        </tr>
      </TableCardBody>

      <hr className="my-0" />

      <div className="card-body py-3">
        <div className="align-box-row align-items-start justify-content-between">
          <div className="d-flex justify-content-between w-100">
            <span className="mb-0 font-size-sm">TPS history</span>

            <div className="font-size-sm">
              {SERIES.map((key) => (
                <button
                  key={key}
                  onClick={() => setSeries(key)}
                  className={classNames("btn btn-sm btn-white ms-2", {
                    active: series === key,
                  })}
                >
                  {SERIES_INFO[key].interval}
                </button>
              ))}
            </div>
          </div>

          <div
            id="perf-history"
            className="mt-3 d-flex justify-content-end flex-row w-100"
          >
            <div className="w-100">
              <Bar data={chartData} options={chartOptions} height={80} />
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

function AnimatedTransactionCount({ info }: { info: PerformanceInfo }) {
  const txCountRef = React.useRef(0);
  const countUpRef = React.useRef({ start: 0, period: 0, lastUpdate: 0 });
  const countUp = countUpRef.current;

  const { transactionCount: txCount, avgTps } = info;

  // Track last tx count to reset count up options
  if (txCount !== txCountRef.current) {
    if (countUp.lastUpdate > 0) {
      // Since we overshoot below, calculate the elapsed value
      // and start from there.
      const elapsed = Date.now() - countUp.lastUpdate;
      const elapsedPeriods = elapsed / (PERF_UPDATE_SEC * 1000);
      countUp.start = Math.floor(
        countUp.start + elapsedPeriods * countUp.period
      );

      // if counter gets ahead of actual count, just hold for a bit
      // until txCount catches up (this will sometimes happen when a tab is
      // sent to the background and/or connection drops)
      countUp.period = Math.max(txCount - countUp.start, 1);
    } else {
      // Since this is the first tx count value, estimate the previous
      // tx count in order to have a starting point for our animation
      countUp.period = PERF_UPDATE_SEC * avgTps;
      countUp.start = txCount - countUp.period;
    }
    countUp.lastUpdate = Date.now();
    txCountRef.current = txCount;
  }

  // Overshoot the target tx count in case the next update is delayed
  const COUNT_PERIODS = 3;
  const countUpEnd = countUp.start + COUNT_PERIODS * countUp.period;
  return (
    <CountUp
      start={countUp.start}
      end={countUpEnd}
      duration={PERF_UPDATE_SEC * COUNT_PERIODS}
      delay={0}
      useEasing={false}
      preserveValue={true}
      separator=","
    />
  );
}
