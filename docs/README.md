# Docs Readme

Solana's Docs are built using [Docusaurus 2](https://v2.docusaurus.io/) with `npm`.
Static content delivery is handled using `vercel`.

### Installation

```
$ npm install
```

### Local Development

```
$ npm run start
```

This command starts a local development server and open up a browser window. Most changes are reflected live without having to restart the server.

### Build
#### Local Build Testing
```
$ npm run build
```

This command generates static content into the `build` directory and can be
served using any static contents hosting service.

#### CI Build Flow
The docs are built and published in Travis CI with the `docs/build.sh` script.
On each PR, the docs are built, but not published.

In each post-commit build, docs are built and published using `vercel` to their
respective domain depending on the build branch.

 - Master branch docs are published to `edge.docs.solana.com`
 - Beta branch docs are published to `beta.docs.solana.com`
 - Latest release tag docs are published to `docs.solana.com`
