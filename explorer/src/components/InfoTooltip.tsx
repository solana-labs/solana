import React, { useState, ReactNode } from "react";

type Props = {
  text: string;
  children: ReactNode;
  bottom?: boolean;
  right?: boolean;
};

type State = "hide" | "show";

function Popover({
  state,
  bottom,
  right,
  text
}: {
  state: State;
  bottom?: boolean;
  right?: boolean;
  text: string;
}) {
  if (state === "hide") return null;
  return (
    <div
      className={`popover bs-popover-${bottom ? "bottom" : "top"}${
        right ? " right" : ""
      } show`}
    >
      <div className={`arrow${right ? " right" : ""}`} />
      <div className="popover-body">{text}</div>
    </div>
  );
}

function InfoTooltip({ bottom, right, text, children }: Props) {
  const [state, setState] = useState<State>("hide");

  const justify = right ? "end" : "start";
  return (
    <div
      className="popover-container w-100"
      onMouseOver={() => setState("show")}
      onMouseOut={() => setState("hide")}
    >
      <div className={`d-flex align-items-center justify-content-${justify}`}>
        {children}
        <span className="fe fe-help-circle ml-2"></span>
      </div>
      <Popover bottom={bottom} right={right} state={state} text={text} />
    </div>
  );
}

export default InfoTooltip;
