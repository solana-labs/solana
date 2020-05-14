import React from "react";

export default function LoadingCard() {
  return (
    <div className="card">
      <div className="card-body text-center">
        <span className="spinner-grow spinner-grow-sm mr-2"></span>
        Loading
      </div>
    </div>
  );
}
