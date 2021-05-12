---
title: "Tổng quát"
---

Một [ứng dụng](terminology.md#app) tương tác với một cụm Solana bằng cách gửi cho nó các [giao dịch](transactions.md) với một hoặc nhiều [hướng dẫn](transactions.md#instructions). [Thời gian chạy](runtime.md) Solana chuyển các hướng dẫn đó đến các [chương trình](terminology.md#program) đã được các nhà phát triển ứng dụng triển khai trước đó. Ví dụ, một chỉ dẫn có thể yêu cầu một chương trình chuyển các [lamport](terminology.md#lamports) từ một [tài khoản](accounts.md) này sang một tài khoản khác hoặc tạo một hợp đồng tương tác điều chỉnh cách chuyển các lamport. Các hướng dẫn được thực hiện tuần tự và nguyên tử cho mỗi giao dịch. Nếu bất kỳ hướng dẫn nào không hợp lệ, tất cả các thay đổi tài khoản trong giao dịch sẽ bị loại bỏ.

Để bắt đầu phát triển ngay lập tức, bạn có thể xây dựng, triển khai và chạy một trong các [ví dụ](developing/deployed-programs/examples.md).