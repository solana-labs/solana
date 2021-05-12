---
title: Hướng dẫn về ví Solana
---

Tài liệu này mô tả các tùy chọn ví khác nhau có sẵn cho người dùng Solana, người dùng có thể gửi, nhận và tương tác với mã thông báo SOL trên blockchain Solana.

## Ví là gì?

Ví tiền điện tử là một thiết bị hoặc ứng dụng lưu trữ một bộ sưu tập khóa và có thể được sử dụng để gửi, nhận, và theo dõi quyền sở hữu tiền điện tử. Ví có thể có nhiều dạng. Ví có thể là một thư mục hoặc tệp trong hệ thống tệp của máy tính của bạn, một mảnh giấy hoặc một thiết bị chuyên dụng được gọi là _ví phần cứng_. Ngoài ra còn có nhiều ứng dụng trên điện thoại thông minh và chương trình máy tính khác cũng cung cấp cách tạo và quản lý ví thân thiện với người dùng.

_keypair_ là một _private key_ được tạo an toàn và _public key_ có nguồn gốc mã hóa của nó. Private key và public key tương ứng của nó được gọi là _keypair_. Ví chứa một bộ sưu tập của một hoặc nhiều keypair và cung cấp một số phương tiện để tương tác với chúng.

_public key_ (được viết tắt là _pubkey_</em>) được gọi là _địa chỉ nhận_ của ví hoặc đơn giản là _địa chỉ_ của nó. Địa chỉ ví **có thể được chia sẻ và hiển thị công khai**. Khi một bên khác sẽ gửi một số lượng tiền điện tử vào ví, họ cần biết địa chỉ nhận của ví. Tùy thuộc vào việc triển khai của blockchain, địa chỉ cũng có thể được sử dụng để xem thông tin nhất định về ví, chẳng hạn như xem số dư, nhưng không có khả năng thay đổi bất kỳ điều gì về ví hoặc rút bất kỳ mã thông báo nào.

_private key_ được yêu cầu để ký giao dịch để gửi tiền điện tử đến một địa chỉ khác hoặc để thực hiện bất kỳ thay đổi nào đối với ví. **Tuyệt đối không chia sẻ** private key. Nếu ai đó có quyền truy cập vào private key của ví, họ có thể rút tất cả các mã thông báo trong ví đó. Nếu private key của ví bị mất, bất kỳ mã thông báo nào đã được gửi đến địa chỉ của ví đó sẽ bị mất vĩnh viễn.

Các giải pháp ví khác nhau cung cấp các cách tiếp cận khác nhau để bảo mật keypair và tương tác với keypair và ký giao dịch để sử dụng/chi tiêu mã thông báo. Một số dễ sử dụng hơn những cái khác. Một số lưu trữ và sao lưu private key an toàn hơn. Solana hỗ trợ nhiều loại ví để bạn có thể lựa chọn và cân nhắc về tính bảo mật và tiện lợi.

**Nếu bạn muốn nhận mã thông báo SOL trên blockchain Solana, trước tiên bạn cần tạo một ví.**

## Ví được hỗ trợ

Solana hỗ trợ một số loại ví trong ứng dụng dòng lệnh gốc Solana cũng như ví từ các bên thứ ba.

Đối với đa số người dùng, chúng tôi khuyên bạn nên sử dụng [ví ứng dụng](wallet-guide/apps.md) hoặc [ví web](wallet-guide/web-wallets.md) dựa trên trình duyệt, điều này sẽ cung cấp trải nghiệm người dùng quen thuộc hơn thay vì cần học các công cụ dòng lệnh.

Đối với người dùng hoặc nhà phát triển nâng cao, [ví dòng lệnh](wallet-guide/cli.md) có thể sẽ phù hợp hơn, vì các tính năng mới trên blockchain Solana sẽ luôn được hỗ trợ trên dòng lệnh trước khi được tích hợp vào các giải pháp của bên thứ ba.
