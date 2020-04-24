import React from "react";

type OverlayProps = {
  show: boolean;
};

export default function Overlay({ show }: OverlayProps) {
  return <div className={`modal-backdrop fade${show ? " show" : ""}`}></div>;
}
