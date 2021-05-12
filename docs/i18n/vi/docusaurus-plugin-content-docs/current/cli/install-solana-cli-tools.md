---
title: Cài đặt Bộ công cụ Solana
---

Có nhiều cách để cài đặt các công cụ Solana trên máy tính của bạn tùy thuộc vào quy trình làm việc ưa thích của bạn:

- [Sử dụng Công cụ cài đặt của Solana (Tùy chọn đơn giản nhất)](#use-solanas-install-tool)
- [Tải xuống Binaries dựng sẵn](#download-prebuilt-binaries)
- [Xây dựng từ nguồn](#build-from-source)

## Sử dụng Công cụ cài đặt của Solana

### MacOS & Linux

- Mở ứng dụng Terminal yêu thích của bạn

- Cài đặt bản phát hành Solana [LATEST_SOLANA_RELEASE_VERSION](https://github.com/solana-labs/solana/releases/tag/LATEST_SOLANA_RELEASE_VERSION) trên máy của bạn bằng cách chạy:

```bash
sh -c "$(curl -sSfL https://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/install)"
```

- Bạn có thể thay thế `LATEST_SOLANA_RELEASE_VERSION` bằng tên phiên bản phát hành phù hợp với phiên bản cài đặt của bạn, hoặc sử dụng một trong ba cái tên mang tính biểu tượng sau: `stable`, `beta`, hoặc `edge`.

- Kết quả sau cho biết cập nhật thành công:

```text
downloading LATEST_SOLANA_RELEASE_VERSION installer
Configuration: /home/solana/.config/solana/install/config.yml
Active release directory: /home/solana/.local/share/solana/install/active_release
* Release version: LATEST_SOLANA_RELEASE_VERSION
* Release URL: https://github.com/solana-labs/solana/releases/download/LATEST_SOLANA_RELEASE_VERSION/solana-release-x86_64-unknown-linux-gnu.tar.bz2
Update successful
```

- Tùy thuộc vào hệ thống của bạn, thông báo kết thúc trình cài đặt có thể nhắc bạn

```bash
Please update your PATH environment variable to include the solana programs:
```

- Nếu bạn nhận được thông báo trên, hãy sao chép và dán lệnh được đề xuất bên dưới để cập nhật `PATH`
- Xác nhận bạn đã cài đặt phiên bản `solana` mong muốn bằng cách chạy:

```bash
solana --version
```

- Sau khi cài đặt thành công, `solana-install update` có thể được sử dụng để dễ dàng cập nhật phần mềm Solana lên phiên bản mới hơn bất kỳ lúc nào.

---

### Windows

- Mở Command Prompt (`cmd.exe`) với tư cách là Administrator

  - Tìm kiếm Command Prompt trong thanh tìm kiếm của Windows. Khi ứng dụng Command Prompt xuất hiện, hãy nhấp chuột phải và chọn “Open as Administrator”. Nếu bạn được nhắc bởi cửa sổ bật lên hỏi “Do you want to allow this app to make changes to your device?”, Hãy nhấp vào Yes.

- Sao chép và dán lệnh sau, sau đó nhấn Enter để tải xuống trình cài đặt Solana vào thư mục tạm thời:

```bash
curl https://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/solana-install-init-x86_64-pc-windows-msvc.exe --output C:\solana-install-tmp\solana-install-init.exe --create-dirs
```

- Sao chép và dán lệnh sau, sau đó nhấn Enter để cài đặt phiên bản Solana mới nhất. Nếu bạn thấy cửa sổ của hệ thống bật lên, vui lòng chọn cho phép chương trình chạy.

```bash
C:\solana-install-tmp\solana-install-init.exe LATEST_SOLANA_RELEASE_VERSION
```

- Khi cài đặt hoàn tất, hãy nhấn Enter.

- Đóng cửa sổ nhắc lệnh và mở lại cửa sổ nhắc lệnh mới như bình thường
  - Tìm kiếm "Command Prompt" trong thanh tìm kiếm, sau đó nhấp chuột trái vào biểu tượng ứng dụng Command Prompt, không cần chạy với tư cách Administrator)
- Xác nhận bạn đã cài đặt phiên bản `solana` mong muốn bằng cách nhập:

```bash
solana --version
```

- Sau khi cài đặt thành công, `solana-install update` có thể được sử dụng để dễ dàng cập nhật phần mềm Solana lên phiên bản mới bất kỳ lúc nào.

## Tải xuống Binaries dựng sẵn

Nếu bạn không muốn sử dụng `solana-install` để quản lý cài đặt, bạn có thể tải xuống và cài đặt các tệp nhị phân theo cách thủ công.

### Linux

Tải xuống tệp nhị phân bằng cách nhấn vào đây [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), tải xuống **solana-release-x86_64-unknown-linux-msvc.tar.bz2**, sau đó tiến hành giải nén:

```bash
tar jxf solana-release-x86_64-unknown-linux-gnu.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### MacOS

Tải xuống tệp nhị phân bằng cách nhấn vào đây [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), tải xuống **solana-release-x86_64-apple-darwin.tar.bz2**, sau đó tiến hành giải nén:

```bash
tar jxf solana-release-x86_64-apple-darwin.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### Windows

- Tải xuống tệp nhị phân bằng cách nhấn vào đây [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), tải xuống **solana-release-x86_64-pc-windows-msvc.tar.bz2**, sau đó giải nén bằng WinZip hoặc phần mềm tương tự.

- Mở Command Prompt và nhấn vào thư mục mà bạn đã giải nén các tệp nhị phân và chạy:

```bash
cd solana-release/
set PATH=%cd%/bin;%PATH%
```

## Xây dựng từ nguồn

Nếu bạn không thể sử dụng các tệp nhị phân được tạo sẵn hoặc muốn tự xây dựng nó từ nguồn, hãy nhấn vào [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), và tải xuống **Source Code**. Giải nén và xây dựng các tệp nhị phân với:

```bash
./scripts/cargo-install-all.sh .
export PATH=$PWD/bin:$PATH
```

Sau đó, bạn có thể chạy lệnh sau để nhận được kết quả tương tự như với các tệp nhị phân dựng sẵn:

```bash
solana-install init
```
