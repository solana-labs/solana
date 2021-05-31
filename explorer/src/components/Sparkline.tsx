import React from "react";
import { Line } from "react-chartjs-2";

// From _chart.scss
export const CHART_WIDTH = 75;
export const CHART_HEIGHT = 35;

export const POSITIVE_COLOR = "#26e97e";
export const NEGATIVE_COLOR = "#fa62fc";

export enum Direction {
  Positive,
  Negative,
}

export function Sparkline({
  values,
  direction,
}: {
  values: Array<number>;
  direction: Direction;
}) {
  const options = {
    responsive: false,
    legend: {
      display: false,
    },
    elements: {
      line: {
        borderColor:
          direction === Direction.Positive ? POSITIVE_COLOR : NEGATIVE_COLOR,
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
  };
  const data = {
    labels: values,
    datasets: [
      {
        data: values,
      },
    ],
  };

  return (
    <Line
      width={CHART_WIDTH}
      height={CHART_HEIGHT}
      data={data}
      options={options}
    />
  );
}
