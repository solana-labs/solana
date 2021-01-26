<p align="center">
  <a href="https://solana.com">
    <img alt="Solana" src="https://i.imgur.com/OMnvVEz.png" width="250" />
  </a>
</p>

[![Solana crate](https://img.shields.io/crates/v/solana-core.svg)](https://crates.io/crates/solana-core)
[![Solana documentation](https://docs.rs/solana-core/badge.svg)](https://docs.rs/solana-core)
[![Build status](https://badge.buildkite.com/8cc350de251d61483db98bdfc895b9ea0ac8ffa4a32ee850ed.svg?branch=master)](https://buildkite.com/solana-labs/solana/builds?branch=master)
[![codecov](https://codecov.io/gh/solana-labs/solana/branch/master/graph/badge.svg)](https://codecov.io/gh/solana-labs/solana)

# 搭建

## **1. 安裝 rustc、cargo 以及 rustfmt**

```bash
$ curl https://sh.rustup.rs -sSf | sh
$ source $HOME/.cargo/env
$ rustup component add rustfmt
```

請通過以下命令來確保使用的 Rust 版本為「最新穩定版」:

```bash
$ rustup update
```

在 Linux 系統上，您可能需要安裝 libssl-dev、pkg-config、zlib1g-dev 等套件。 如在 Ubuntu 上:

```bash
$ sudo apt-get update
$ sudo apt-get install libssl-dev libudev-dev pkg-config zlib1g-dev llvm clang make
```

## **2. 下載原始碼**

```bash
$ git clone https://github.com/solana-labs/solana.git
$ cd solana
```

## **3. 搭建**

```bash
$ cargo build
```

## **4. 運行最小的本地群集(節點)**

```bash
$ ./run.sh
```

# 測試

**運行測試套件:**

```bash
$ cargo test
```

### 運行本地測試網
在本地運行您自己的測試網。([說明文檔](https://docs.solana.com/cluster/bench-tps))

### 訪問遠程開發集群
* `devnet` - 24小時在線的穩定公用集群，可通過 devnet.solana.com 訪問。更多說明請參閱 [說明文檔](https://docs.solana.com/clusters)

# 基準測試 (Benchmarking)

請先安裝 nightly 版本的 rustc。 `cargo bench` 提供的不穩定版功能僅能在 nightly 版本中搭建。

```bash
$ rustup install nightly
```

運行 nightly 版分支:

```bash
$ cargo +nightly bench
```

# 發布版本歷史

可以在 [這裡](RELEASE.md) 查閱此項目的版本歷史與說明。

# 代碼覆蓋率

自動產生代碼覆蓋率統計:

```bash
$ scripts/coverage.sh
$ open target/cov/lcov-local/index.html
```

Why coverage? While most see coverage as a code quality metric, we see it primarily as a developer
productivity metric. When a developer makes a change to the codebase, presumably it's a *solution* to
some problem.  Our unit-test suite is how we encode the set of *problems* the codebase solves. Running
the test suite should indicate that your change didn't *infringe* on anyone else's solutions. Adding a
test *protects* your solution from future changes. Say you don't understand why a line of code exists,
try deleting it and running the unit-tests. The nearest test failure should tell you what problem
was solved by that code. If no test fails, go ahead and submit a Pull Request that asks, "what
problem is solved by this code?" On the other hand, if a test does fail and you can think of a
better way to solve the same problem, a Pull Request with your solution would most certainly be
welcome! Likewise, if rewriting a test can better communicate what code it's protecting, please
send us that patch!

# 免責聲明

All claims, content, designs, algorithms, estimates, roadmaps,
specifications, and performance measurements described in this project
are done with the Solana Foundation's ("SF") best efforts. It is up to
the reader to check and validate their accuracy and truthfulness.
Furthermore nothing in this project constitutes a solicitation for
investment.

Any content produced by SF or developer resources that SF provides, are
for educational and inspiration purposes only. SF does not encourage,
induce or sanction the deployment, integration or use of any such
applications (including the code comprising the Solana blockchain
protocol) in violation of applicable laws or regulations and hereby
prohibits any such deployment, integration or use. This includes use of
any such applications by the reader (a) in violation of export control
or sanctions laws of the United States or any other applicable
jurisdiction, (b) if the reader is located in or ordinarily resident in
a country or territory subject to comprehensive sanctions administered
by the U.S. Office of Foreign Assets Control (OFAC), or (c) if the
reader is or is working on behalf of a Specially Designated National
(SDN) or a person subject to similar blocking or denied party
prohibitions.

The reader should be aware that U.S. export control and sanctions laws
prohibit U.S. persons (and other persons that are subject to such laws)
from transacting with persons in certain countries and territories or
that are on the SDN list. As a project based primarily on open-source
software, it is possible that such sanctioned persons may nevertheless
bypass prohibitions, obtain the code comprising the Solana blockchain
protocol (or other project code or applications) and deploy, integrate,
or otherwise use it. Accordingly, there is a risk to individuals that
other persons using the Solana blockchain protocol may be sanctioned
persons and that transactions with such persons would be a violation of
U.S. export controls and sanctions law. This risk applies to
individuals, organizations, and other ecosystem participants that
deploy, integrate, or use the Solana blockchain protocol code directly
(e.g., as a node operator), and individuals that transact on the Solana
blockchain through light clients, third party interfaces, and/or wallet
software.







