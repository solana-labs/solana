import { useState } from "react";
import { displayTimestampUtc, displayTimestamp } from "utils/date";

type State = "hide" | "show";

function Tooltip({ state }: { state: State }) {
  const tooltip = {
    maxWidth: "20rem",
  };

  if (state === "hide") return null;
  return (
    <div className="popover bs-popover-bottom show" style={tooltip}>
      <div className="arrow" />
      <div className="popover-body">
        (Click to toggle between local and UTC)
      </div>
    </div>
  );
}

export function TimestampToggle({ unixTimestamp }: { unixTimestamp: number }) {
  const [isTimestampTypeUtc, toggleTimestampType] = useState(true);
  const [showTooltip, toggleTooltip] = useState<State>("hide");

  const toggle = () => {
    toggleTimestampType(!isTimestampTypeUtc);
  };

  const tooltipContainer = {
    cursor: "pointer",
  };

  return (
    <div className="popover-container w-100" style={tooltipContainer}>
      <span
        onClick={toggle}
        onMouseOver={() => toggleTooltip("show")}
        onMouseOut={() => toggleTooltip("hide")}
      >
        {isTimestampTypeUtc === true
          ? displayTimestampUtc(unixTimestamp)
          : displayTimestamp(unixTimestamp)}
      </span>
      <Tooltip state={showTooltip} />
    </div>
  );
}
