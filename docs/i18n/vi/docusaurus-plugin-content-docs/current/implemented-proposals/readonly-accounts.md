---
title: Tài khoản chỉ-đọc
---

Thiết kế này bao gồm việc xử lý các tài khoản chỉ đọc và có thể ghi trong [thời gian chạy](../validator/runtime.md). Nhiều giao dịch sửa đổi cùng một tài khoản phải được xử lý tuần tự để chúng luôn được phát lại theo cùng một thứ tự. Nếu không, điều này có thể dẫn đến tính không-xác định đối với sổ cái. Tuy nhiên, một số giao dịch chỉ cần đọc và không cần sửa đổi dữ liệu trong các tài khoản cụ thể. Nhiều giao dịch chỉ đọc cùng một tài khoản có thể được xử lý song song, vì thứ tự phát lại không quan trọng, mang lại lợi ích về hiệu suất.

Để xác định các tài khoản chỉ đọc, cấu trúc MessageHeader giao dịch chứa `num_readonly_signed_accounts` và `num_readonly_unsigned_accounts`. Lệnh `program_ids` được bao gồm trong vector tài khoản dưới dạng tài khoản chỉ đọc, chưa ký, vì các tài khoản thực thi tương tự như vậy không thể được sửa đổi trong quá trình xử lý lệnh.

## Xử lý thời gian chạy

Các quy tắc xử lý giao dịch thời gian chạy cần được cập nhật một chút. Các chương trình vẫn không thể ghi hoặc chi tiêu các tài khoản mà họ không sở hữu. Nhưng các quy tắc thời gian chạy mới đảm bảo rằng không thể sửa đổi các tài khoản chỉ đọc, ngay cả bởi các chương trình sở hữu chúng.

Tài khoản chỉ đọc có thuộc tính sau:

- Quyền truy cập chỉ-đọc vào tất cả các trường tài khoản, bao gồm cả các lamport (không thể ghi có hoặc ghi nợ) và dữ liệu tài khoản

Hướng dẫn rằng ghi có, ghi nợ hoặc sửa đổi tài khoản chỉ đọc sẽ không thành công.

## Tối ưu hóa khóa tài khoản

Mô-đun Tài khoản theo dõi các tài khoản bị khóa hiện tại trong thời gian chạy, phân tách tài khoản chỉ đọc khỏi tài khoản có thể ghi. Khóa tài khoản mặc định cung cấp cho một tài khoản chỉ định "có thể ghi" và chỉ có thể được truy cập bởi một luồng xử lý tại một thời điểm. Các tài khoản chỉ đọc được khóa bởi một cơ chế riêng biệt, cho phép đọc song song.

Mặc dù chưa được triển khai, các tài khoản chỉ đọc có thể được lưu trong bộ nhớ và được chia sẻ giữa tất cả các luồng thực hiện giao dịch. Một thiết kế lý tưởng sẽ giữ bộ nhớ đệm này trong khi tài khoản chỉ đọc được tham chiếu bởi bất kỳ giao dịch nào di chuyển qua thời gian chạy và giải phóng bộ nhớ đệm khi giao dịch cuối cùng thoát khỏi thời gian chạy.

Các tài khoản chỉ đọc cũng có thể được chuyển vào bộ xử lý dưới dạng tài liệu tham khảo, tiết kiệm thêm một bản sao.
