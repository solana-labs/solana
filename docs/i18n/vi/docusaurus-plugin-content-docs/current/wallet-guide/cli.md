---
title: Ví dòng lệnh
---

Solana hỗ trợ một số loại ví khác nhau, để có thể được sử dụng để giao tiếp trực tiếp với các công cụ dòng lệnh Solana.

**Nếu bạn không quen sử dụng các chương trình dòng lệnh và chỉ muốn có thể gửi và nhận mã thông báo SOL, chúng tôi khuyên bạn nên thiết lập Ví ứng dụng của bên thứ ba [Ví Ứng dụng](apps.md)**.

Để sử dụng Ví dòng lệnh, trước tiên bạn phải [cài đặt các công cụ Solana CLI](../cli/install-solana-cli-tools.md)

## Ví hệ thống tệp

_Ví hệ thống tệp_, hay còn được gọi là ví FS, là một thư mục trong hệ thống máy tính của bạn. Mỗi tệp trong thư mục chứa một keypair.

### Bảo mật Ví Hệ thống Tệp

Ví hệ thống tệp là hình thức ví tiện lợi nhất mà cũng kém an toàn nhất. Nó thuận tiện vì keypair được lưu trữ trong một file đơn giản. Bạn có thể tạo bao nhiêu khóa tùy thích và sao lưu chúng bằng cách sao chép các file. Nó không an toàn vì các tệp keypair **không được mã hóa**. Nếu bạn là người dùng duy nhất trên máy tính của mình và bạn tin tưởng rằng nó không có phần mềm độc hại, ví FS là một giải pháp tốt cho số lượng nhỏ tiền điện tử. Tuy nhiên, nếu máy tính của bạn chứa phần mềm độc hại và được kết nối với Internet, phần mềm độc hại đó có thể tải lên các khóa của bạn và sử dụng chúng để lấy mã thông báo của bạn. Tương tự như vậy, vì các keypair được lưu trữ trên máy tính của bạn dưới dạng file, một tin tặc có tay nghề sẽ truy cập vật lý vào máy tính của bạn có thể truy cập vào nó. Sử dụng ổ cứng được mã hóa, chẳng hạn như FileVault trên MacOS, sẽ giảm thiểu rủi ro đó.

[Ví Hệ thống Tệp](file-system-wallet.md)

## Ví giấy

_Ví giấy_ là tập hợp _các cụm từ hạt giống_ được viết trên giấy. Cụm từ hạt giống là một số từ (thường là 12 hoặc 24) có thể được sử dụng để tạo keypair theo yêu cầu.

### Bảo mật Ví giấy

Về mặt tiện lợi so với bảo mật, ví giấy nằm ở phía đối diện với ví FS. Nó rất bất tiện khi sử dụng, nhưng mang lại tính bảo mật tuyệt vời. Tính bảo mật cao đó càng được tăng cường khi ví giấy được sử dụng cùng với [việc ký ngoại tuyến](../offline-signing.md). Các dịch vụ lưu ký như [Coinbase Custody](https://custody.coinbase.com/) sử dụng kết hợp này. Ví giấy và dịch vụ lưu ký là một cách tuyệt vời để bảo vệ một số lượng lớn mã thông báo trong khoảng thời gian dài.

[Ví giấy](paper-wallet.md)

## Ví Phần cứng

Ví phần cứng là một thiết bị cầm tay nhỏ lưu trữ các keypair và cung cấp một số giao diện để ký giao dịch.

### Bảo mật ví phần cứng

Ví phần cứng, chẳng hạn như [ví phần cứng Ledger](https://www.ledger.com/), cung cấp sự kết hợp tuyệt vời giữa bảo mật và tiện lợi cho tiền điện tử. Nó tự động hóa hiệu quả của quá trình ký ngoại tuyến trong khi vẫn giữ được gần như tất cả sự tiện lợi của ví hệ thống tệp.

[Ví phần cứng](hardware-wallets.md)
