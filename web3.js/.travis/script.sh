# |source| this file

ls -l lib
test -r lib/index.iife.js
test -r lib/index.cjs.js
test -r lib/index.esm.js
npm run doc
npm run defs
npm run flow
npm run lint
npm run codecov
make -C examples/bpf-c-noop/
examples/bpf-rust-noop/do.sh build
npm run localnet:update
npm run localnet:up
npm run test:live
npm run localnet:down
