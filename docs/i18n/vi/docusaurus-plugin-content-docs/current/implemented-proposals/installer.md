---
title: Cài đặt và cập nhật phần mềm Cụm
---

Hiện tại, người dùng bắt buộc phải tự xây dựng phần mềm cụm solana từ kho lưu trữ git và cập nhật thủ công, điều này dễ xảy ra lỗi và bất tiện.

Tài liệu này đề xuất một trình cập nhật và cài đặt phần mềm dễ sử dụng có thể được sử dụng để triển khai các tệp nhị phân được tạo sẵn cho các nền tảng được hỗ trợ. Người dùng có thể chọn sử dụng các mã nhị phân do Solana hoặc bất kỳ bên nào khác mà họ tin tưởng cung cấp. Việc triển khai các bản cập nhật được quản lý bằng cách sử dụng chương trình kê khai cập nhật trực tuyến.

## Ví dụ về động lực

### Tìm nạp và chạy trình cài đặt được tạo sẵn bằng tập lệnh bootstrap curl/shell

Phương pháp cài đặt dễ dàng nhất cho các nền tảng được hỗ trợ:

```bash
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh | sh
```

Tập lệnh này sẽ kiểm tra github để tìm bản phát hành được gắn thẻ mới nhất và tải xuống và chạy tệp nhị phân `solana-install-init` từ đó.

Nếu các đối số bổ sung cần được chỉ định trong quá trình cài đặt, thì cú pháp shell sau sẽ được sử dụng:

```bash
$ init_args=.... # arguments for `solana-install-init ...`
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh | sh -s - ${init_args}
```

### Tìm nạp và chạy trình cài đặt được tạo sẵn từ bản phát hành Github

Với URL phát hành nổi tiếng, có thể lấy bản nhị phân được tạo sẵn cho các nền tảng được hỗ trợ:

```bash
$ curl -o solana-install-init https://github.com/solana-labs/solana/releases/download/v1.0.0/solana-install-init-x86_64-apple-darwin
$ chmod +x ./solana-install-init
$ ./solana-install-init --help
```

### Xây dựng và chạy trình cài đặt từ nguồn

Nếu một bản nhị phân được tạo sẵn không có sẵn cho một nền tảng nhất định, thì việc xây dựng trình cài đặt từ nguồn luôn là một tùy chọn:

```bash
$ git clone https://github.com/solana-labs/solana.git
$ cd solana/install
$ cargo run -- --help
```

### Triển khai bản cập nhật mới cho một cụm

Với bản phát hành solana tarball \(như được tạo bởi `ci/publish-tarball.sh`\) đã được tải lên URL có thể truy cập công khai, các lệnh sau sẽ triển khai bản cập nhật:

```bash
$ solana-keygen new -o update-manifest.json  # <-- only generated once, the public key is shared with users
$ solana-install deploy http://example.com/path/to/solana-release.tar.bz2 update-manifest.json
```

### Chạy một node validator tự động cập nhật

```bash
$ solana-install init --pubkey 92DMonmBYXwEMHJ99c9ceRSpAmk9v6i3RdvDdXaVcrfj  # <-- pubkey is obtained from whoever is deploying the updates
$ export PATH=~/.local/share/solana-install/bin:$PATH
$ solana-keygen ...  # <-- runs the latest solana-keygen
$ solana-install run solana-validator ...  # <-- runs a validator, restarting it as necesary when an update is applied
```

## Tệp kê khai cập nhật trên chuỗi

Tệp kê khai cập nhật được sử dụng để quảng cáo việc triển khai các tarball phát hành mới trên một cụm solana. Tệp kê khai cập nhật được lưu trữ bằn cách sử dụng chương trình `config` và mỗi tài khoản kê khai cập nhật mô tả một kênh cập nhật hợp lý cho bộ ba mục tiêu nhất định \(ví dụ: `x86_64-apple-darwin`\). Public key của tài khoản được biết đến giữa tổ chức triển khai các bản cập nhật mới và người dùng sử dụng các bản cập nhật đó.

Bản thân bản cập nhật tarball được lưu trữ ở nơi khác, ngoài chuỗi và có thể được tìm nạp từ `download_url` chỉ định.

```text
use solana_sdk::signature::Signature;

/// Information required to download and apply a given update
pub struct UpdateManifest {
    pub timestamp_secs: u64, // When the release was deployed in seconds since UNIX EPOCH
    pub download_url: String, // Download URL to the release tar.bz2
    pub download_sha256: String, // SHA256 digest of the release tar.bz2 file
}

/// Data of an Update Manifest program Account.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct SignedUpdateManifest {
    pub manifest: UpdateManifest,
    pub manifest_signature: Signature,
}
```

Lưu ý rằng bản thân trường `manifest` chứa một chữ ký tương ứng \(`manifest_signature`\) để bảo vệ chống lại các cuộc tấn công man-in-the-middle giữa công cụ `solana-install` và API RPC của cụm solana.

Để bảo vệ chống lại các cuộc tấn công khôi phục, `solana-install` sẽ từ chối cài đặt bản cập nhật có `timestamp_secs` cũ hơn những gì hiện được cài đặt.

## Phát hành nội dung lưu trữ

Bản lưu trữ phát hành dự kiến ​​là một tệp tar được nén bằng bzip2 với cấu trúc bên trong là:

- `/version.yml` - một tệp YAML đơn giản chứa trường `"target"`

  mục tiêu tuple. Mọi trường bổ sung đều bị bỏ qua.

- `/bin/` -- thư mục chứa các chương trình có sẵn trong bản phát hành.

  `solana-install` sẽ liên kết biểu tượng thư mục này với

  `~/.local/share/solana-install/bin` để sử dụng bởi môi trường `PATH`

  biến đổi.

- `...` -- bất kỳ tệp và thư mục bổ sung nào đều được phép

## công cụ cài đặt solana

`solana-install` công cụ này được người dùng sử dụng để cài đặt và cập nhật phần mềm cụm của họ.

Nó quản lý các tệp và thư mục sau trong thư mục chính của người dùng:

- `~/.config/solana/install/config.yml` - cấu hình người dùng và thông tin về phiên bản phần mềm hiện được cài đặt
- `~/.local/share/solana/install/bin` - một liên kết tượng trưng cho bản phát hành hiện tại. ví dụ, `~/.local/share/solana-update/<update-pubkey>-<manifest_signature>/bin`
- `~/.local/share/solana/install/releases/<download_sha256>/` - nội dung của một bản phát hành

### Giao diện dòng lệnh

```text
solana-install 0.16.0
The solana cluster software installer

USAGE:
    solana-install [OPTIONS] <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -c, --config <PATH>    Configuration file to use [default: .../Library/Preferences/solana/install.yml]

SUBCOMMANDS:
    deploy    deploys a new update
    help      Prints this message or the help of the given subcommand(s)
    info      displays information about the current installation
    init      initializes a new installation
    run       Runs a program while periodically checking and applying software updates
    update    checks for an update, and if available downloads and applies it
```

```text
solana-install-init
initializes a new installation

USAGE:
    solana-install init [OPTIONS]

FLAGS:
    -h, --help    Prints help information

OPTIONS:
    -d, --data_dir <PATH>    Directory to store install data [default: .../Library/Application Support/solana]
    -u, --url <URL>          JSON RPC URL for the solana cluster [default: http://devnet.solana.com]
    -p, --pubkey <PUBKEY>    Public key of the update manifest [default: 9XX329sPuskWhH4DQh6k16c87dHKhXLBZTL3Gxmve8Gp]
```

```text
solana-install info
displays information about the current installation

USAGE:
    solana-install info [FLAGS]

FLAGS:
    -h, --help     Prints help information
    -l, --local    only display local information, don't check the cluster for new updates
```

```text
solana-install deploy
deploys a new update

USAGE:
    solana-install deploy <download_url> <update_manifest_keypair>

FLAGS:
    -h, --help    Prints help information

ARGS:
    <download_url>               URL to the solana release archive
    <update_manifest_keypair>    Keypair file for the update manifest (/path/to/keypair.json)
```

```text
solana-install update
checks for an update, and if available downloads and applies it

USAGE:
    solana-install update

FLAGS:
    -h, --help    Prints help information
```

```text
solana-install run
Runs a program while periodically checking and applying software updates

USAGE:
    solana-install run <program_name> [program_arguments]...

FLAGS:
    -h, --help    Prints help information

ARGS:
    <program_name>            program to run
    <program_arguments>...    arguments to supply to the program

The program will be restarted upon a successful software update
```
