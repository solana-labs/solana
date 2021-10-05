# Docs Readme

Solana's Docs are built using [Docusaurus 2](https://v2.docusaurus.io/) with `npm`.
Static content delivery is handled using `vercel`.

### Installing Docusaurus

```sh
$ npm install
```

### Local Development

This command starts a local development server and opens up a browser window.
Most changes are reflected live without having to restart the server.
(You might have to run build.sh first if you run into failures)

```sh
$ npm run start
```

### Build Locally

This command generates static content into the `build` directory and can be
served using any static content hosting service.

```sh
$ docs/build.sh
```

### Translations

Translations are sourced from [Crowdin](https://docusaurus.io/docs/i18n/crowdin)
and generated when master is built.
For local development use the following two commands in the `docs` directory.

To download the newest Documentation translations run:

```sh
npm run crowdin:download
```

To upload changes from `src` & generate [explicit IDs](https://docusaurus.io/docs/markdown-features/headings#explicit-ids):

```shell
npm run crowdin:upload
```

### CI Build Flow

The docs are built and published in Travis CI with the `docs/build.sh` script.
On each PR, the docs are built, but not published.

In each post-commit build, docs are built and published using `vercel` to their
respective domain depending on the build branch.

- Master branch docs are published to `edge.docs.solana.com`
- Beta branch docs are published to `beta.docs.solana.com`
- Latest release tag docs are published to `docs.solana.com`
