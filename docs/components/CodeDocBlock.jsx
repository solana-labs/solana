import React from "react";
import Link from "@docusaurus/Link";
// import clsx from "clsx";
// import styles from "../src/pages/styles.module.css";

export function DocBlock({ children }) {
  return (
    <section
      className=""
      style={{
        borderTop: "1px solid #414141",
        paddingTop: "3rem",
        // display: "flex",
        // justifyContent: "space-between",
        marginTop: "5rem",
        // width: "100%",
        // "align-items": "center",
      }}
    >
      {children}
    </section>
  );
}

export function DocSideBySide({ children }) {
  return (
    <section
      className=""
      style={{
        display: "flex",
        justifyContent: "space-between",
        marginTop: "2rem",
        // width: "100%",
        // "align-items": "center",
      }}
    >
      {children}
    </section>
  );
}

export function CodeParams({ children }) {
  return (
    <section
      className=""
      style={{
        display: "block",
        // "background-color": "red",
        marginRight: "3.0rem",
        width: "50%",
      }}
    >
      {children}
    </section>
  );
}

export function CodeSnippets({ children }) {
  return (
    <section
      className=""
      style={{
        display: "block",
        // "background-color": "red",
        width: "50%",
      }}
    >
      {/* <p
        style={{
          fontSize: "1.24rem",
          fontWeight: "700",
        }}
      >
        Code Sample:
      </p> */}

      {children}
    </section>
  );
}

export function Parameter({
  name = null,
  type = null,
  required = null,
  optional = null,
  href = null,
  children,
}) {
  // format the Parameter's name
  // if (name) name = <code>{name}</code>;
  // format the Parameter's name
  if (name) {
    name = <span style={{ fontWeight: "700" }}>{name}</span>;

    if (href) name = <Link href={href}>{name}</Link>;
  }

  // format the Parameter's type
  if (type) type = <code>{type}</code>;

  // format the `required` flag
  if (required) {
    required = (
      <span
        style={{
          margin: "0 .5rem",
          color: "#767676",
          fontWeight: "600",
        }}
      >
        required
      </span>
    );
  }
  // format the `optional` flag
  else if (optional) {
    optional = (
      <span
        style={{
          margin: "0 .5rem",
          color: "#767676",
          fontWeight: "600",
        }}
      >
        optional
      </span>
    );
  }

  return (
    <section
      style={{
        padding: "1em 0em",
        marginBottom: "1em",
        borderTop: "1px solid #414141",
        // borderBottom: "1px solid #414141",
      }}
    >
      <p
        style={{
          fontFamily: "mono",
        }}
      >
        {name && name} {type && type} {required && required}{" "}
        {optional && optional}
      </p>

      {children}
    </section>
  );
}

export function Field({
  name = null,
  type = null,
  href = null,
  values = null,
  required = null,
  defaultValue = null,
  optional = null,
  children,
}) {
  // format the Parameter's name
  if (name) {
    name = <span style={{ fontWeight: "700" }}>{name}</span>;

    if (href) name = <Link href={href}>{name}</Link>;
  }

  // format the Parameter's type
  if (type) type = <code>{type}</code>;

  // format the Parameter's values
  if (values && Array.isArray(values)) {
    values = values.map((value) => (
      <code style={{ marginRight: ".5em" }}>{value}</code>
    ));
  }

  // format the `defaultValue` flag
  if (defaultValue) {
    defaultValue = (
      <span
        style={{
          margin: "0 .5rem",
          color: "#767676",
          fontWeight: "600",
        }}
      >
        Default: <code>{defaultValue.toString()}</code>
      </span>
    );
  }

  // format the `required` flag
  if (required) {
    required = (
      <span
        style={{
          margin: "0 .5rem",
          color: "#767676",
          fontWeight: "600",
        }}
      >
        required
      </span>
    );
  }
  // format the `optional` flag
  else if (optional) {
    optional = (
      <span
        style={{
          margin: "0 .5rem",
          color: "#767676",
          fontWeight: "600",
        }}
      >
        optional
      </span>
    );
  }

  return (
    <section
      style={{
        // padding: "1em 0em",
        margin: "1em 0em 1em 1em",
        // borderTop: "1px solid #414141",
        // borderBottom: "1px solid #414141",
      }}
    >
      <p
        style={{
          fontFamily: "mono",
          padding: ".1em 0em",
        }}
      >
        {name && name} {type && type} {required && required}{" "}
        {optional && optional}
        {defaultValue && defaultValue}
      </p>

      <section
        style={{
          padding: "0em 1em",
        }}
      >
        {values && values}

        {children}
      </section>
    </section>
  );
}

export function Values({ values = null }) {
  // format the Parameter's values
  if (values && Array.isArray(values)) {
    values = values.map((value) => (
      <code style={{ marginRight: ".5em" }} key={value}>
        {value}
      </code>
    ));
  }

  // return <p style={{}}>{values}</p>;
  return (
    <p style={{}}>
      <span
        style={{
          marginRight: "1rem",
          // color: "#767676",
          fontWeight: "600",
        }}
      >
        Values:
      </span>{" "}
      {values}
    </p>
  );
}
