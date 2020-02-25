Building the Solana Docs
---

Install the book's dependencies, build, and test the book:

```bash
$ brew install coreutils
$ cargo install svgbob_cli
$ brew install mscgen
$ brew install mdbook
$ ./build.sh
```

Run any Rust tests in the markdown:

```bash
$ make test
```

Render markdown as HTML:

```bash
$ make build
```

Render and view the book:

```bash
$ make open
```
