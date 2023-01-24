import React from "react";
import Link from "@docusaurus/Link";
// import clsx from "clsx";
import styles from "../src/pages/CodeDocBlock.module.css";

export function DocBlock({ children }) {
  return <section className={styles.DocBlock}>{children}</section>;
}

export function DocSideBySide({ children }) {
  return <section className={styles.DocSideBySide}>{children}</section>;
}

export function CodeParams({ children }) {
  return <section className={styles.CodeParams}>{children}</section>;
}

export function CodeSnippets({ children }) {
  return (
    <section className={styles.CodeSnippets}>
      {/* <p className={styles.Heading}>Code Sample:</p> */}

      {children}
    </section>
  );
}

/*
  Display a single Parameter
*/
export function Parameter(props) {
  const {
    name = null,
    type = null,
    required = null,
    optional = null,
    children,
  } = computeHeader(props);

  return (
    <section className={styles.Parameter}>
      <p className={styles.ParameterHeader}>
        {name && name} {type && type} {required && required}{" "}
        {optional && optional}
      </p>

      {children}
    </section>
  );
}

/*
  Display a single Parameter's field data
*/
export function Field(props) {
  const {
    name = null,
    type = null,
    values = null,
    required = null,
    defaultValue = null,
    optional = null,
    children,
  } = computeHeader(props);

  return (
    <section className={styles.Field}>
      <p className={styles.ParameterHeader}>
        {name && name} {type && type} {required && required}{" "}
        {optional && optional}
        {defaultValue && defaultValue}
      </p>

      <section>
        {values && values}

        {children}
      </section>
    </section>
  );
}

/*
  Parse an array of string values to display
*/
export function Values({ values = null }) {
  // format the Parameter's values
  if (values && Array.isArray(values) && values?.length) {
    values = values.map((value) => (
      <code style={{ marginRight: ".5em" }} key={value}>
        {value}
      </code>
    ));
  }

  return (
    <p style={{}}>
      <span className={styles.SubHeading}>Values:</span>&nbsp;{values}
    </p>
  );
}

/*
  Compute the formatted Parameter and Field component's header meta data
*/
function computeHeader({
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
    name = <span className={styles.ParameterName}>{name}</span>;

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
      <span className={styles.FlagItem}>
        Default: <code>{defaultValue.toString()}</code>
      </span>
    );
  }

  // format the `required` flag
  if (required) {
    required = <span className={styles.FlagItem}>required</span>;
  }
  // format the `optional` flag
  else if (optional) {
    optional = <span className={styles.FlagItem}>optional</span>;
  }

  return {
    name,
    type,
    href,
    values,
    required,
    defaultValue,
    optional,
    children,
  };
}
