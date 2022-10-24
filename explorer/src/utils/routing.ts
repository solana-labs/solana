import { useRouter } from "next/router";
import { Cluster, clusterSlug, useCluster } from "providers/cluster";
import React from "react";

/**
 * Placeholder URL usage is to act as a baseURL for URL function to parse
 * locations, params and hashes for client-side routing. This is to
 * avoid having to call window.location.origin and verify if it is
 * server-sided or client-side props.
 */
const PLACEHOLDER_URL = "https://solana.com";

export class Route {
  public hash: string;

  static parse(path: string) {
    const url = new URL(path, PLACEHOLDER_URL);
    return new Route(url.pathname, url.searchParams, url.hash);
  }

  constructor(
    public pathname: string,
    public searchParams: URLSearchParams,
    hash?: string
  ) {
    this.hash = hash || "";
  }

  toString() {
    let path = this.pathname;
    const search = this.searchParams.toString();
    if (search.length > 0) {
      path += `?${search}`;
    }
    return path;
  }
}

export function useCurrentRoute() {
  const router = useRouter();
  return React.useMemo(() => {
    return Route.parse(router.asPath);
  }, [router]);
}

export function useSearchParams() {
  const currentRoute = useCurrentRoute();
  return currentRoute.searchParams;
}

export type ClusterPathCreator = (
  pathname: string,
  params?: URLSearchParams
) => string;

export const useCreateClusterPath = (): ClusterPathCreator => {
  const { cluster, customUrl } = useCluster();
  return React.useCallback(
    (pathname: string, params?: URLSearchParams): string => {
      const newRoute = new Route(pathname, params || new URLSearchParams());

      if (cluster !== Cluster.MainnetBeta) {
        newRoute.searchParams.set("cluster", clusterSlug(cluster));
      }
      if (customUrl && cluster === Cluster.Custom) {
        newRoute.searchParams.set("customUrl", customUrl);
      }

      return newRoute.toString();
    },
    [cluster, customUrl]
  );
};
