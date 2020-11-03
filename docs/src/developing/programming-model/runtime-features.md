---
title: "Runtime Features"
---

As Solana evolves, new features or patches may be introduced that changes the
behavior of the cluster and how programs run.  Changes in behavior must be
coordinated between the various nodes of the cluster, if nodes do not coordinate
then these changes can result in a break-down of consensus.  Solana supports a
mechanism called runtime features to facilitate the smooth adoption of changes.

Runtime features are epoch coordinated events where one or more behavior changes
to the cluster will occur.  New changes to Solana that will change behavior are
wrapped with feature gates and disabled by default.  The Solana tools are then
used to activate a feature, which marks it pending, once marked pending the
feature will be activated at the next epoch.

To determine which features are activated use the [Solana command-line
tools](cli/install-solana-cli-tools.md):

```bash
solana feature status
```

If you encounter problems first ensure that the Solana tools version you are
using match the version returned by `solana cluster-version`.  If they do not
match [install the correct tool suite](cli/install-solana-cli-tools.md).
