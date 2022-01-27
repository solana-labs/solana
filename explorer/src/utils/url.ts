import { useLocation } from "react-router-dom";
import { Location } from "history";

export function useQuery() {
  return new URLSearchParams(useLocation().search);
}

export const clusterPath = (pathname: string, params?: URLSearchParams) => {
  return (location: Location) => ({
    ...pickClusterParams(location, params),
    pathname,
  });
};

export function pickClusterParams(
  location: Location,
  newParams?: URLSearchParams
): Location {
  const urlParams = new URLSearchParams(location.search);
  const cluster = urlParams.get("cluster");
  const customUrl = urlParams.get("customUrl");

  // Pick the params we care about
  newParams = newParams || new URLSearchParams();
  if (cluster) newParams.set("cluster", cluster);
  if (customUrl) newParams.set("customUrl", customUrl);

  return {
    ...location,
    search: newParams.toString(),
  };
}
interface URIStrategy {
  id: string,
  test: (uri: string) => boolean,
  transform: (uri: string) => Promise<any> | string
}
 
const uriProtocolStrategies: URIStrategy[] = [
  {
    id: 'ipfs',
    test: (uri: string) => uri.indexOf("ipfs://") > -1,
    transform: (uri: string) => {
      return uri.replace("ipfs://", "https://ipfs.io/ipfs/");
    },
  },
  {
    id: 'ar',
    test: (uri: string) => uri.indexOf("ar://") > -1,
    transform: async (uri: string) => {
      try {
        let arUrl = uri.replace("ar://", "https://arweave.net/");
        const data = await fetch(arUrl).then(res => res.json()).catch(console.error)
        if(!data.image) return uri
        return data.image
      } catch (e) {
       return uri
      }
    }
  },
  {
    id: 'http',
    test: (uri: string) => uri.indexOf("http://") > -1,
    transform : (uri: string) => uri,
  },
  {
    id: 'https',
    test: (uri: string) => uri.indexOf("https://") > -1,
    transform : (uri: string) => uri
  }
]

function getUriStrategy(uri: string, strategies: URIStrategy[]) {
  return strategies.reduce((acc: URIStrategy | null, strategy: URIStrategy, index) => {
    if (acc == null) {
      return strategy.test(uri) ? strategy : acc;
    } else {
      return acc;
    }
  }, null);
}

export async function getDecentralizedURI(uri: string): Promise<string> {

  const strategy: URIStrategy | null = await getUriStrategy(uri, uriProtocolStrategies);
    
  if(!strategy) return uri

  const _out = strategy.transform(uri)
  return _out || uri
}