import React from "react";

export function LoadingCard({ message }: { message?: string }) {
  return (
    <div className="card">
      <div className="card-body text-center">
        <span className="spinner-grow spinner-grow-sm me-2"></span>
        {message || "Loading"}
      </div>
    </div>
  );
}
