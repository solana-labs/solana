---
title: Kết nối với một Cụm
---

Xem [Cụm Solana](../clusters.md) để biết thêm thông tin chung về các cụm có sẵn.

## Cấu hình công cụ dòng lệnh

Bạn có thể kiểm tra cụm công cụ dòng lệnh Solana (CLI) bằng cách chạy lệnh sau:

```bash
solana config get
```

Sử dụng lệnh `solana config set` để nhắm mục tiêu một cụm cụ thể. Sau khi đặt mục tiêu cụm, mọi lệnh con nào trong tương lai sẽ gửi/nhận thông tin từ cụm đó.

Ví dụ: để nhắm mục tiêu cụm Devnet, hãy chạy:

```bash
solana config set --url https://devnet.solana.com
```

## Đảm bảo các phiên bản phù hợp

Mặc dù không hoàn toàn cần thiết, CLI nói chung sẽ hoạt động tốt nhất khi phiên bản của nó khớp với phiên bản phần mềm đang chạy trên cụm. Để tải phiên bản CLI được cài đặt cục bộ, hãy chạy:

```bash
solana --version
```

Để tải phiên bản cụm, hãy chạy:

```bash
solana cluster-version
```

Đảm bảo phiên bản CLI cục bộ lớn hơn hoặc bằng phiên bản cụm.
