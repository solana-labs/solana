import React, { Children } from "react";
import { useRouter } from "next/router";
import cx from "classnames";
import Link, { LinkProps } from "next/link";

type NavLinkProps = React.PropsWithChildren<LinkProps> & {
  activeClassName?: string;
};

export const NavLink = ({
  children,
  activeClassName = "active",
  ...props
}: NavLinkProps) => {
  const { asPath } = useRouter();

  const child = Children.only(children) as React.ReactElement;
  const childClassName = child.props.className || "";

  const [pathWithoutParams] = asPath.split('?');
  const [hrefWithoutParams] = (props.href as string).split('?');
  const isActive = pathWithoutParams === hrefWithoutParams || pathWithoutParams === props.as;

  const className = cx(childClassName, { [activeClassName]: isActive }, 'c-pointer');

  return (
    <Link {...props}>
      {React.cloneElement(child, {
        className: className || null
      })}
    </Link>
  );
};
