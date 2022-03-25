# |source| this file

set -ex
solana --version

npm run clean
npm run build
ls -l lib
test -r lib/index.iife.js
test -r lib/index.cjs.js
test -r lib/index.esm.js
# npm run ok
# npm run codecov
START_SERVER_AND_TEST_INSECURE=true DEBUG=start-server-and-test npm run test:live-with-test-validator
