# Solana Coding Guidelines

The goal of these guidelines is to improve developer productivity by allowing
developers to jump into any file in the codebase and not need to adapt to
inconsistencies in how the code is written. The codebase should appear as if it
had been authored by a single developer. If you don't agree with a convention,
submit a PR patching this document and let's discuss! Once the PR is accepted,
*all* code should be updated as soon as possible to reflect the new
conventions.

## Pull Requests

Small, frequent PRs are much preferred to large, infrequent ones. A large PR is
difficult to review, can block others from making progress, and can quickly get
its author into "rebase hell". A large PR oftentimes arises when one change
requires another, which requires another, and then another. When you notice
those dependencies, put the fix into a commit of its own, then checkout a new
branch, and cherry-pick it.

```bash
$ git commit -am "Fix foo, needed by bar"
$ git checkout master
$ git checkout -b fix-foo
$ git cherry-pick fix-bar
$ git push --set-upstream origin fix-foo
```

Open a PR to start the review process and then jump back to your original
branch to keep making progress. Consider rebasing to make your fix the first
commit:

```bash
$ git checkout fix-bar
$ git rebase -i master <Move fix-foo to top>
```

Once the commit is merged, rebase the original branch to purge the
cherry-picked commit:

```bash
$ git pull --rebase upstream master
```

### How big is too big?

If there are no functional changes, PRs can be very large and that's no
problem. If, however, your changes are making meaningful changes or additions,
then about 1,000 lines of changes is about the most you should ask a Solana
maintainer to review.

### Should I send small PRs as I develop large, new components?

Add only code to the codebase that is ready to be deployed. If you are building
a large library, consider developing it in a separate git repository. When it
is ready to be integrated, the Solana Labs Maintainers will work with you to decide
on a path forward. Smaller libraries may be copied in whereas very large ones
may be pulled in with a package manager.

## Getting Pull Requests Merged

There is no single person assigned to watching GitHub PR queue and ushering you
through the process. Typically, you will ask the person that wrote a component
to review changes to it. You can find the author using `git blame` or asking on
Discord.  When working to get your PR merged, it's most important to understand
that changing the code is your priority and not necessarily a priority of the
person you need an approval from. Also, while you may interact the most with
the component author, you should aim to be inclusive of others. Providing a
detailed problem description is the most effective means of engaging both the
component author and other potentially interested parties.

Consider opening all PRs as Draft Pull Requests first. Using a draft PR allows
you to kickstart the CI automation, which typically takes between 10 and 30
minutes to execute. Use that time to write a detailed problem description. Once
the description is written and CI succeeds, click the "Ready to Review" button
and add reviewers. Adding reviewers before CI succeeds is a fast path to losing
reviewer engagement. Not only will they be notified and see the PR is not yet
ready for them, they will also be bombarded with additional notifications
each time you push a commit to get past CI or until they "mute" the PR. Once
muted, you'll need to reach out over some other medium, such as Discord, to
request they have another look. When you use draft PRs, no notifications are
sent when you push commits and edit the PR description. Use draft PRs
liberally.  Don't bug the humans until you have gotten past the bots.

### What should be in my PR description?

Reviewing code is hard work and generally involves an attempt to guess the
author's intent at various levels. Please assume reviewer time is scarce and do
what you can to make your PR as consumable as possible. Inspired by techniques
for writing good whitepapers, the guidance here aims to maximize reviewer
engagement.

Assume the reviewer will spend no more than a few seconds reading the PR title.
If it doesn't describe a noteworthy change, don't expect the reviewer to click
to see more.

Next, like the abstract of a whitepaper, the reviewer will spend ~30 seconds
reading the PR problem description. If what is described there doesn't look
more important than competing issues, don't expect the reviewer to read on.

Next, the reviewer will read the proposed changes. At this point, the reviewer
needs to be convinced the proposed changes are a *good* solution to the problem
described above.  If the proposed changes, not the code changes, generates
discussion, consider closing the PR and returning with a design proposal
instead.

Finally, once the reviewer understands the problem and agrees with the approach
to solving it, the reviewer will view the code changes. At this point, the
reviewer is simply looking to see if the implementation actually implements
what was proposed and if that implementation is maintainable. When a concise,
readable test for each new code path is present, the reviewer can safely ignore
the details of its implementation. When those tests are missing, expect to
either lose engagement or get a pile of review comments as the reviewer
attempts to consider every ambiguity in your implementation.

### The PR Title

The PR title should contain a brief summary of the change, from the perspective
of the user. Examples of good titles:

* Add rent to accounts
* Fix out-of-memory error in validator
* Clean up `process_message()` in runtime

The conventions here are all the same as a good git commit title:

* First word capitalized and in the imperative mood, not past tense ("add", not
  "added")
* No trailing period
* What was done, whom it was done to, and in what context

### The PR Problem Statement

The git repo implements a product with various features. The problem statement
should describe how the product is missing a feature, how a feature is
incomplete, or how the implementation of a feature is somehow undesirable. If
an issue being fixed already describes the problem, go ahead and copy-paste it.
As mentioned above, reviewer time is scarce. Given a queue of PRs to review,
the reviewer may ignore PRs that expect them to click through links to see if
the PR warrants attention.

### The Proposed Changes

Typically the content under the "Proposed changes" section will be a bulleted
list of steps taken to solve the problem. Oftentimes, the list is identical to
the subject lines of the git commits contained in the PR. It's especially
generous (and not expected) to rebase or reword commits such that each change
matches the logical flow in your PR description.

### The PR / Issue Labels

Labels make it easier to manage and track PRs / issues.  Below some common labels
that we use in Solana.  For the complete list of labels, please refer to the
[label page](https://github.com/solana-labs/solana/issues/labels):

* "feature-gate": when you add a new feature gate or modify the behavior of
an existing feature gate, please add the "feature-gate" label to your PR.
New feature gates should also always have a corresponding tracking issue
(go to "New Issue" -> "Feature Gate Tracker [Get Started](https://github.com/solana-labs/solana/issues/new?assignees=&labels=feature-gate&template=1-feature-gate.yml&title=Feature+Gate%3A+)")
and should be updated each time the feature is activated on a cluster.

* "automerge": When a PR is labelled with "automerge", the PR will be
automically merged once CI passes.  In general, this label should only
be used for small hot-fix (fewer than 100 lines) or automatic generated
PRs.  If you're uncertain, it's usually the case that the PR is not
qualified as "automerge".

* "good first issue": If you happen to find an issue that is non-urgent and
self-contained with moderate scope, you might want to consider attaching
"good first issue" to it as it might be a good practice for newcomers.

### When will my PR be reviewed?

PRs are typically reviewed and merged in under 7 days. If your PR has been open
for longer, it's a strong indicator that the reviewers aren't confident the
change meets the quality standards of the codebase. You might consider closing
it and coming back with smaller PRs and longer descriptions detailing what
problem it solves and how it solves it. Old PRs will be marked stale and then
closed automatically 7 days later.

### How to manage review feedback?

After a reviewer provides feedback, you can quickly say "acknowledged, will
fix" using a thumb's up emoji. If you're confident your fix is exactly as
prescribed, add a reply "Fixed in COMMIT\_HASH" and mark the comment as
resolved. If you're not sure, reply "Is this what you had in mind?
COMMIT\_HASH" and if so, the reviewer will reply and mark the conversation as
resolved. Marking conversations as resolved is an excellent way to engage more
reviewers. Leaving conversations open may imply the PR is not yet ready for
additional review.

### When will my PR be re-reviewed?

Recall that once your PR is opened, a notification is sent every time you push
a commit.  After a reviewer adds feedback, they won't be checking on the status
of that feedback after every new commit. Instead, directly mention the reviewer
when you feel your PR is ready for another pass.

### Is your PR easy to say "yes" to?

PRs that are easier to review are more likely to be reviewed. Strive to make
your PR easy to say "yes" to.

Non-exhaustive list of things that make it *harder* to review:

* Additional changes that are orthogonal to the problem statement and proposed
  changes. Instead move those changes to a different PR.
* Renaming variables/functions/types unnecessarily and/or without explanation.
* Not following established conventions in the function/module/crate/repo.
* Changing whitespace: moving code and/or reformatting code. Make such changes
  in a separate PR.
* Force-pushing the branch unnecessarily; this makes it harder to track any
  previous comments on specific lines of code, and also harder to track changes
  already reviewed from previous commits.
  * When force-pushing is required—for example to handle a merge conflict—and
    no new changes have been made since the previous review, indicating as such
    is beneficial.
 * Not responding to comments from previous rounds of review. Follow the
   guidance in [How to manage review feedback?](#how-to-manage-review-feedback).

Non-exhaustive list of things that make it *easier* to review:

* Adding tests for all new/changed behavior.
* Including in the PR's description any non-automated testing that was
  performed.
* Including relevant results for changes that target performance improvements.

Note that these lists are *independent* of how simple/complicated the actual
*code* changes are.

## Draft Pull Requests

If you want early feedback on your PR, use GitHub's "Draft Pull Request"
mechanism. Draft PRs are a convenient way to collaborate with the Solana
maintainers without triggering notifications as you make changes. When you feel
your PR is ready for a broader audience, you can transition your draft PR to a
standard PR with the click of a button.

Do not add reviewers to draft PRs.  GitHub doesn't automatically clear
approvals when you click "Ready for Review", so a review that meant "I approve
of the direction" suddenly has the appearance of "I approve of these changes."
Instead, add a comment that mentions the usernames that you would like a review
from. Ask explicitly what you would like feedback on.

## Crate Creation

If your PR includes a new crate, you must publish its v0.0.1 version
before the PR can be merged.  Here are the steps:

* Create a sub-directory for your new crate.
* Under the newly-created directory, create a Cargo.toml file.  Below is an
  example template:

```toml
[package]
name = "solana-<PACKAGE_NAME>"
version = "0.0.1"
description = "<DESCRIPTION>"
authors = ["Solana Labs Maintainers <maintainers@solanalabs.com>"]
repository = "https://github.com/solana-labs/solana"
homepage = "https://solana.com/"
documentation = "https://docs.rs/solana-<PACKAGE_NAME>"
license = "Apache-2.0"
edition = "2021"
```

* Submit the PR for initial review.  You should see the crate-check CI
  job fails because the newly created crate is not yet published.

* Once all review feedback has been addressed, publish v0.0.1 of the crate
  under your personal crates.io account, and then transfer the crate ownership
  to solana-grimes.
  https://crates.io/policies#package-ownership

* After successful publication, update the PR by replacing the v0.0.1 version
  number with the correct version.  At this time you should see the crate-check
  CI job passes, and your published crate should be available under
  https://crates.io/crates/.

## Rust coding conventions

* All Rust code is formatted using the latest version of `rustfmt`. Once
  installed, it will be updated automatically when you update the compiler with
`rustup`.

* All Rust code is linted with Clippy. If you'd prefer to ignore its advice, do
  so explicitly:

  ```rust
  #[allow(clippy::too_many_arguments)]
  ```

  Note: Clippy defaults can be overridden in the top-level file `.clippy.toml`.

* For variable names, when in doubt, spell it out. The mapping from type names
  to variable names is to lowercase the type name, putting an underscore before
each capital letter. Variable names should *not* be abbreviated unless being
used as closure arguments and the brevity improves readability. When a function
has multiple instances of the same type, qualify each with a prefix and
underscore (i.e. alice\_keypair) or a numeric suffix (i.e. tx0).

* For function and method names, use `<verb>_<subject>`. For unit tests, that
  verb should always be `test` and for benchmarks the verb should always be
`bench`. Avoid namespacing function names with some arbitrary word. Avoid
abbreviating words in function names.

* As they say, "When in Rome, do as the Romans do." A good patch should
  acknowledge the coding conventions of the code that surrounds it, even in the
case where that code has not yet been updated to meet the conventions described
here.


## Terminology

Inventing new terms is allowed, but should only be done when the term is widely
used and understood. Avoid introducing new 3-letter terms, which can be
confused with 3-letter acronyms.

[Terms currently in use](docs/src/terminology.md)


## Design Proposals

Solana's architecture is described by docs generated from markdown files in the `docs/src/`
directory and viewable on the official [Solana Documentation](https://docs.solana.com) website.

Current design proposals may be viewed on the docs site:

1. [Accepted Proposals](https://docs.solana.com/proposals/accepted-design-proposals)
2. [Implemented Proposals](https://docs.solana.com/implemented-proposals/implemented-proposals)

New design proposals should follow this guide on [how to submit a design proposal](./docs/src/proposals.md#submit-a-design-proposal).
