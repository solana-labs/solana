import React from "react";
import Card from "./Card";

export default function HomeCtaLinks() {
  return (
    <div className="container">
      <div className="row cards__container">
        <Card
          to="https://solana.com/developers"
          header={{
            label: "Developers",
            translateId: "cta-developers",
          }}
        />

        <Card
          to="operations"
          header={{
            label: "Operate a Validator",
            translateId: "cta-validators",
          }}
        />

        <Card
          to="clusters"
          header={{
            label: "Architecture",
            translateId: "cta-architecture",
          }}
        />
      </div>
    </div>
  );
}
