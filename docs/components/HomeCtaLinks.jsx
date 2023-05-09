import React from "react";
import Card from "./Card";

export default function HomeCtaLinks() {
  return (
    <div className="container">
      <div className="row cards__container">
        <Card
          to="developers"
          header={{
            label: "Developers",
            translateId: "cta-developers",
          }}
        />

        <Card
          to="running-validator"
          header={{
            label: "Validators",
            translateId: "cta-validators",
          }}
        />

        <Card
          to="cluster/overview"
          header={{
            label: "Architecture",
            translateId: "cta-architecture",
          }}
        />
      </div>
    </div>
  );
}
