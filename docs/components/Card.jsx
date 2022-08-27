import React from "react";
import clsx from "clsx";
import Link from "@docusaurus/Link";
import styles from "../src/pages/styles.module.css";
import Translate from "@docusaurus/Translate";

function Card({ to, header, body }) {
  /* 
  Both the `header` and `body` expect an object with the following type
  header = {
    label: String, //
    translateId: String //
  }
  */

  return (
    <div className={clsx("col col--4 ", styles.feature)}>
      <Link className="navbar__link card" to={to}>
        <div className="card__header">
          <h3>
            <Translate description={header.translateId}>
              {header.label}
            </Translate>
          </h3>
        </div>
        <div className="card__body">
          <p>
            <Translate description={body.translateId}>{body.label}</Translate>
          </p>
        </div>
      </Link>
    </div>
  );
}

export default Card;
