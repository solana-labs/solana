import { useRouter } from 'next/router'
import InspectorPage from 'pages/tx/inspector'

export function TransactionInspectorPage() {
	const router = useRouter()
	const signature = router.query.signature as string | undefined

  return <InspectorPage signature={signature} />
}

export default TransactionInspectorPage
