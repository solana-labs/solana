# |source| this file

set -ex
solana --version

ls -l lib
test -r lib/index.iife.js
test -r lib/index.cjs.js
test -r lib/index.esm.js
npm run doc
npm run lint
npm run codecov
npm run test:live-with-test-validator
npm run test:browser-with-test-validator
