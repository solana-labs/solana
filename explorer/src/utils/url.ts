import { useRouter } from 'next/router'
import { dummyUrl } from 'src/constants/urls'

export function useQuery() {
	const router = useRouter()

	if (typeof window === 'undefined')
		return new URLSearchParams()

	const location = new URL(router.asPath, dummyUrl)
	return new URLSearchParams(location.search)
}

export const clusterPath = (
	pathname: string,
	routerPath: string,
	params?: URLSearchParams
): string => {
	const location = new URL(routerPath, dummyUrl)
  const newParams = pickClusterParams(location, params)

  return newParams.length > 0
    ? `${pathname}?${newParams}`
    : pathname
}

export function pickClusterParams(
	location: URL,
	newParams?: URLSearchParams
): string {
	const urlParams = new URLSearchParams(location.search)
	const cluster = urlParams.get('cluster')
	const customUrl = urlParams.get('customUrl')

	// Pick the params we care about
	newParams = newParams || new URLSearchParams()
	if (cluster) newParams.set('cluster', cluster)
	if (customUrl) newParams.set('customUrl', customUrl)

	return newParams.toString()
}
