import React from "react";

type OverlayProps = {
  show: boolean;
};

export function Overlay({ show }: OverlayProps) {
  return (
    <div
      className={`modal-backdrop fade ${
        show ? "show" : "disable-pointer-events"
      }`}
    ></div>
  );
}
