# Security Policy

1. [Reporting security problems](#reporting)
4. [Security Bug Bounties](#bounty)
2. [Incident Response Process](#process)

<a name="reporting"></a>
## Reporting security problems to Solana

**DO NOT CREATE AN ISSUE** to report a security problem. Instead, please send an
email to security@solana.com and provide your github username so we can add you
to a new draft security advisory for further discussion.

Expect a response as fast as possible, within one business day at the latest.

<a name="bounty"></a>
## Security Bug Bounties
We offer bounties for critical security issues. Please see below for more details.

Loss of Funds:
$2,000,000 USD in locked SOL tokens (locked for 12 months)
* Theft of funds without users signature from any account
* Theft of funds without users interaction in system, token, stake, vote programs
* Theft of funds that requires users signature - creating a vote program that drains the delegated stakes.

Consensus/Safety Violations:
$1,000,000 USD in locked SOL tokens (locked for 12 months)
* Consensus safety violation
* Tricking a validator to accept an optimistic confirmation or rooted slot without a double vote, etc..

Other Attacks:
 $400,000 USD in locked SOL tokens (locked for 12 months)
* Protocol liveness attacks,
* Eclipse attacks,
* Remote attacks that partition the network,

DoS Attacks:
$100,000 USD in locked SOL tokens (locked for 12 months)
* Remote resource exaustion via Non-RPC protocols

RPC DoS/Crashes:
$5,000 USD in locked SOL tokens (locked for 12 months)
* RPC attacks

Out of Scope:
The following components are out of scope for the bounty program
* Metrics: `/metrics` in the monorepo as well as https://metrics.solana.com
* Explorer: `/explorer` in the monorepo as well as https://explorer.solana.com
* Any encrypted credentials, auth tokens, etc. checked into the repo
* Bugs in dependencies. Please take them upstream!
* Attacks that require social engineering

Eligibility:
* The participant submitting the bug report shall follow the process outlined within this document
* Valid exploits can be eligible even if they are not successfully executed on the cluster
* Multiple submissions for the same class of exploit are still eligible for compensation, though may be compensated at a lower rate, however these will be assessed on a case-by-case basis
* Participants must complete KYC and sign the participation agreement here when the registrations are open https://solana.com/validator-registration. Security exploits will still be assessed and open for submission at all times. This needs only be done prior to distribution of tokens.

Payment of Bug Bounties:
* Payments for eligible bug reports are distributed monthly.
* Bounties for all bug reports submitted in a given month are paid out in the middle of the
following month.
* The SOL/USD conversion rate used for payments is the market price at the end of
  the last day of the month for the month in which the bug was submitted.
* The reference for this price is the Closing Price given by Coingecko.com on
  that date given here:
  https://www.coingecko.com/en/coins/solana/historical_data/usd#panel
* For example, for all bugs submitted in March 2021, the SOL/USD price for bug
  payouts is the Close price on 2021-03-31 of $19.49.  This applies to all bugs
  submitted in March 2021, to be paid in mid-April 2021.
* Bug bounties are paid out in
[stake accounts](https://solana.com/staking) with a
[lockup](https://docs.solana.com/staking/stake-accounts#lockups)
expiring 12 months from the last day of the month in which the bug was submitted.

<a name="process"></a>
## Incident Response Process

In case an incident is discovered or reported, the following process will be
followed to contain, respond and remediate:

### 1. Establish a new draft security advisory
In response to an email to security@solana.com, a member of the `solana-labs/admins` group will
1. Create a new draft security advisory for the incident at https://github.com/solana-labs/solana/security/advisories
1. Add the reporter's github user and the `solana-labs/security-incident-response` group to the draft security advisory
1. Create a private fork of the repository (grey button towards the bottom of the page)
1. Respond to the reporter by email, sharing a link to the draft security advisory

### 2. Triage
Within the draft security advisory, discuss and determine the severity of the
issue. If necessary, members of the `solana-labs/security-incident-response`
group may add other github users to the advisory to assist.

If it is determined that this not a critical network issue then the advisory
should be closed and if more follow-up is required a normal Solana public github
issue should be created.

### 3. Prepare Fixes
For the affected branches, typically all three (edge, beta and stable), prepare
a fix for the issue and push them to the corresponding branch in the private
repository associated with the draft security advisory.

There is no CI available in the private repository so you must build from source
and manually verify fixes.

Code review from the reporter is ideal, as well as from multiple members of the
core development team.

### 4. Notify Security Group Validators
Once an ETA is available for the fix, a member of the
`solana-labs/security-incident-response` group should notify the validators so
they can prepare for an update using the "Solana Red Alert" notification system.

The teams are all over the world and it's critical to provide actionable
information at the right time. Don't be the person that wakes everybody up at
2am when a fix won't be available for hours.

### 5. Ship the patch
Once the fix is accepted, a member of the
`solana-labs/security-incident-response` group should prepare a single patch
file for each affected branch. The commit title for the patch should only
contain the advisory id, and not disclose any further details about the
incident.

Copy the patches to https://release.solana.com/ under a subdirectory named after
the advisory id (example:
https://release.solana.com/GHSA-hx59-f5g4-jghh/v1.4.patch). Contact a member of
the `solana-labs/admins` group if you require access to release.solana.com

Using the "Solana Red Alert" channel:
1. Notify validators that there's an issue and a patch will be provided in X minutes
2. If X minutes expires and there's no patch, notify of the delay and provide a
   new ETA
3. Provide links to patches of https://release.solana.com/ for each affected branch

Validators can be expected to build the patch from source against the latest
release for the affected branch.

Since the software version will not change after the patch is applied, request
that each validator notify in the existing channel once they've updated. Manually
monitor the roll out until a sufficient amount of stake has updated - typically
at least 33.3% or 66.6% depending on the issue.

### 6. Public Disclosure and Release
Once the fix has been deployed to the security group validators, the patches from the security
advisory may be merged into the main source repository. A new official release
for each affected branch should be shipped and all validators requested to
upgrade as quickly as possible.

### 7. Security Advisory Bounty Accounting and Cleanup

If this issue is eligible for a bounty, prefix the title of the security
advisory with one of the following, depending on the severity:
* `[Bounty Category: Critical: Loss of Funds]`
* `[Bounty Category: Critical: Loss of Availability]`
* `[Bounty Category: Critical: DoS]`
* `[Bounty Category: Critical: Other]`
* `[Bounty Category: Non-critical]`
* `[Bounty Category: RPC]`

Confirm with the reporter that they agree with the severity assessment, and
discuss as required to reach a conclusion.

We currently do not use the Github workflow to publish security advisories.
Once the issue and fix have been disclosed, and a bounty category is assessed if
appropriate, the GitHub security advisory is no longer needed and can be closed.

Bounties are currently awarded once a quarter (TODO: link to this process, or
inline the workflow)
