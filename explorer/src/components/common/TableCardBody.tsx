import React from "react";

export default function TableCardBody({
  children
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="table-responsive mb-0">
      <table className="table table-sm table-nowrap card-table">
        <tbody className="list">{children}</tbody>
      </table>
    </div>
  );
}
