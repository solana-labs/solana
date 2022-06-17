import React, { Children } from "react";
import { useRouter } from "next/router";
import cx from "classnames";
import Link, { LinkProps } from "next/link";
import { useCluster, Cluster, clusterSlug } from "src/providers/cluster";

type NavLinkProps = React.PropsWithChildren<LinkProps> & {
  activeClassName?: string;
};

export const NavLink = ({
  children,
  activeClassName = "active",
  ...props
}: NavLinkProps) => {
  const { asPath } = useRouter();
  const { cluster } = useCluster();

  const child = Children.only(children) as React.ReactElement;
  const childClassName = child.props.className || "";

  const [pathWithoutParams] = asPath.split('?');
  let isActive: boolean

  if (cluster === Cluster.MainnetBeta) {
    isActive = pathWithoutParams === props.href || pathWithoutParams === props.as;
  } else {
    const pathWithCluster = `${pathWithoutParams}?cluster=${clusterSlug(cluster)}`;
    isActive = pathWithCluster === props.href || pathWithCluster === props.as;
  }

  const className = cx(childClassName, { [activeClassName]: isActive }, 'c-pointer');

  return (
    <Link {...props}>
      {React.cloneElement(child, {
        className: className || null
      })}
    </Link>
  );
};
