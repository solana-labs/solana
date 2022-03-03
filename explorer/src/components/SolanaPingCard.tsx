import React from "react";
import classNames from "classnames";
import {
  PingRollupInfo,
  PingStatus,
  useSolanaPingInfo,
} from "providers/stats/SolanaPingProvider";
import { Bar } from "react-chartjs-2";
import { ChartOptions, ChartTooltipModel } from "chart.js";
import { Cluster, useCluster } from "providers/cluster";

export function SolanaPingCard() {
  const { cluster } = useCluster();

  if (cluster === Cluster.Custom) {
    return null;
  }

  return (
    <div className="card">
      <div className="card-header">
        <h4 className="card-header-title">Solana Ping Stats</h4>
      </div>
      <PingBarBody />
    </div>
  );
}

function PingBarBody() {
  const pingInfo = useSolanaPingInfo();

  if (pingInfo.status !== PingStatus.Ready) {
    return (
      <StatsNotReady
        error={pingInfo.status === PingStatus.Error}
        retry={pingInfo.retry}
      />
    );
  }

  return <PingBarChart pingInfo={pingInfo} />;
}

type StatsNotReadyProps = { error: boolean; retry?: Function };
function StatsNotReady({ error, retry }: StatsNotReadyProps) {
  if (error) {
    return (
      <div className="card-body text-center">
        There was a problem loading solana ping stats.{" "}
        {retry && (
          <button
            className="btn btn-white btn-sm"
            onClick={() => {
              retry();
            }}
          >
            <span className="fe fe-refresh-cw me-2"></span>
            Try Again
          </button>
        )}
      </div>
    );
  }

  return (
    <div className="card-body text-center">
      <span className="spinner-grow spinner-grow-sm me-2"></span>
      Loading
    </div>
  );
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
      let innerHtml = `<div class="value">${value} ms</div>`;
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

const CHART_OPTION: ChartOptions = {
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

function PingBarChart({ pingInfo }: { pingInfo: PingRollupInfo }) {
  const [series, setSeries] = React.useState<Series>("short");
  const seriesData = pingInfo[series] || [];

  const seriesLength = seriesData.length;
  const chartData: Chart.ChartData = {
    labels: seriesData.map((val, i) => {
      return `
      <p class="mb-0">${val.confirmed} of ${val.submitted} confirmed</p>
      ${
        val.loss
          ? `<p class="mb-0">${val.loss.toLocaleString(undefined, {
              style: "percent",
              minimumFractionDigits: 2,
            })} loss</p>`
          : ""
      }
      ${SERIES_INFO[series].label(seriesLength - i)}min ago
      `;
    }),
    datasets: [
      {
        backgroundColor: seriesData.map((val) =>
          val.loss > 0.5 ? "#f00" : "#00D192"
        ),
        hoverBackgroundColor: seriesData.map((val) =>
          val.loss > 0.5 ? "#f00" : "#00D192"
        ),
        borderWidth: 0,
        data: seriesData.map((val) => val.mean || 0),
      },
    ],
  };

  return (
    <div className="card-body py-3">
      <div className="align-box-row align-items-start justify-content-between">
        <div className="d-flex justify-content-between w-100">
          <span className="mb-0 font-size-sm">Average Ping Time</span>

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
            <Bar data={chartData} options={CHART_OPTION} height={80} />
          </div>
        </div>
      </div>
    </div>
  );
}
