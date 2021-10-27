name: Explorer_build&test_on_PR
on:
  pull_request:
    branches:
     - master
    paths:
      - 'explorer/**'
jobs:
  check-explorer:
    runs-on: ubuntu-latest

    defaults:
      run:
        working-directory: explorer
    steps:
      - uses: actions/checkout@v2
        with:
          ref: ${{ github.event.pull_request.head.sha }}
      - uses: actions/setup-node@v2
        with:
          node-version: '14'
          cache: 'npm'
          cache-dependency-path: explorer/package-lock.json
      - run: npm i -g npm@7
      - run: npm ci
      - run: npm run format
      - run: npm run build
      - run: npm run test
