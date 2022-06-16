import React from 'react'
import { useRouter } from 'next/router'

import { ErrorCard } from 'src/components/common/ErrorCard'
import { BlockOverviewCard } from 'src/components/block/BlockOverviewCard'

// IE11 doesn't support Number.MAX_SAFE_INTEGER
const MAX_SAFE_INTEGER = 9007199254740991

export function BlockDetailsPage() {
	const router = useRouter()
	const { slot: queryParams } = router.query
	let slot = ''
	let tab: string | undefined

	if (queryParams) {
		const [receivedSlot, receivedTab] = queryParams
		slot = receivedSlot
		tab = receivedTab
	}

	const slotNumber = Number(slot)
	let output = <ErrorCard text={`Block ${slot} is not valid`} />

	if (
		!isNaN(slotNumber) &&
		slotNumber < MAX_SAFE_INTEGER &&
		slotNumber % 1 === 0
	) {
		output = <BlockOverviewCard slot={slotNumber} tab={tab} />
	}

	return (
		<div className="container mt-n3">
			<div className="header">
				<div className="header-body">
					<h6 className="header-pretitle">Details</h6>
					<h2 className="header-title">Block</h2>
				</div>
			</div>
			{output}
		</div>
	)
}

export default BlockDetailsPage
