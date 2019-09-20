Solana Coding Guidelines
===

The goal of these guidelines is to improve developer productivity by allowing developers to
jump any file in the codebase and not need to adapt to inconsistencies in how the code is
written. The codebase should appear as if it had been authored by a single developer. If you
don't agree with a convention, submit a PR patching this document and let's discuss! Once
the PR is accepted, *all* code should be updated as soon as possible to reflect the new
conventions.

Pull Requests
---

Small, frequent PRs are much preferred to large, infrequent ones. A large PR is difficult
to review, can block others from making progress, and can quickly get its author into
"rebase hell". A large PR oftentimes arises when one change requires another, which requires
another, and then another. When you notice those dependencies, put the fix into a commit of
its own, then checkout a new branch, and cherrypick it. Open a PR to start the review
process and then jump back to your original branch to keep making progress. Once the commit
is merged, you can use git-rebase to purge it from your original branch.

```bash
$ git pull --rebase upstream master
```

### How big is too big?

If there are no functional changes, PRs can be very large and that's no problem. If,
however, your changes are making meaningful changes or additions, then about 1,000 lines of
changes is about the most you should ask a Solana maintainer to review.

### Should I send small PRs as I develop large, new components?

Add only code to the codebase that is ready to be deployed. If you are building a large
library, consider developing it in a separate git repository. When it is ready to be
integrated, the Solana maintainers will work with you to decide on a path forward. Smaller
libraries may be copied in whereas very large ones may be pulled in with a package manager.

### When will my PR be reviewed?

PRs are typically reviewed and merged in under 7 days. If your PR has been open for longer,
it's a strong indicator that the reviewers aren't confident the change meets the quality
standards of the codebase. You might consider closing it and coming back with smaller PRs
and longer descriptions detailing what problem it solves and how it solves it.

Draft Pull Requests
---

If you want early feedback on your PR, use GitHub's "Draft Pull Request"
mechanism. Draft PRs are a convenient way to collaborate with the Solana
maintainers without triggering notifications as you make changes. When you feel
your PR is ready for a broader audience, you can transition your draft PR to a
standard PR with the click of a button.

Do not add reviewers to draft PRs.  GitHub doesn't automatically clear approvals
when you click "Ready for Review", so a review that meant "I approve of the
direction" suddenly has the appearance of "I approve of these changes." Instead,
add a comment that mentions the usernames that you would like a review from. Ask
explicitly what you would like feedback on.

Rust coding conventions
---

* All Rust code is formatted using the latest version of `rustfmt`. Once installed, it will be
  updated automatically when you update the compiler with `rustup`.

* All Rust code is linted with Clippy. If you'd prefer to ignore its advice, do so explicitly:

  ```rust
  #[allow(clippy::too_many_arguments)]
  ```

  Note: Clippy defaults can be overridden in the top-level file `.clippy.toml`.

* For variable names, when in doubt, spell it out. The mapping from type names to variable names
  is to lowercase the type name, putting an underscore before each capital letter. Variable names
  should *not* be abbreviated unless being used as closure arguments and the brevity improves
  readability. When a function has multiple instances of the same type, qualify each with a
  prefix and underscore (i.e. alice_keypair) or a numeric suffix (i.e. tx0).

* For function and method names, use `<verb>_<subject>`. For unit tests, that verb should
  always be `test` and for benchmarks the verb should always be `bench`. Avoid namespacing
  function names with some arbitrary word. Avoid abbreviating words in function names.

* As they say, "When in Rome, do as the Romans do." A good patch should acknowledge the coding
  conventions of the code that surrounds it, even in the case where that code has not yet been
  updated to meet the conventions described here.


Terminology
---

Inventing new terms is allowed, but should only be done when the term is widely used and
understood. Avoid introducing new 3-letter terms, which can be confused with 3-letter acronyms.

[Terms currently in use](book/src/terminology.md)


Design Proposals
---

Solana's architecture is described by a book generated from markdown files in
the `book/src/` directory, maintained by an *editor* (currently @garious). To
add a design proposal, you'll need to at least propose a change the content
under the [Accepted Design
Proposals](https://docs.solana.com/book/v/master/proposals) chapter.
Here's the full process:

1. Propose a design by creating a PR that adds a markdown document to the
   directory `book/src/` and references it from the [table of
   contents](book/src/SUMMARY.md). Add any relevant *maintainers* to the PR review.
2. The PR being merged indicates your proposed change was accepted and that the
   maintainers support your plan of attack.
3. Submit PRs that implement the proposal. When the implementation reveals the
   need for tweaks to the proposal, be sure to update the proposal and have
   that change reviewed by the same people as in step 1.
4. Once the implementation is complete, submit a PR that moves the link from
   the Accepted Proposals to the Implemented Proposals section.
