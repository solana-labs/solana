---
title: Ví Web SolFlare
---

## Giới thiệu

[SolFlare.com](https://solflare.com/) là một ví web do cộng đồng tạo ra được xây dựng đặc biệt cho Solana. SolFlare hỗ trợ gửi và nhận mã thông báo SOL gốc cũng như gửi và nhận mã thông báo SPL (tương đương với ERC-20 của Solana). SolFlare cũng hỗ trợ staking các mã thông báo SOL.

Là một ví _không giám sát_, private key của bạn không được lưu trữ bởi trang web SolFlare mà chúng được lưu trữ trong [Keystore File](#using-a-keystore-file) mã hóa hoặc trên [Ledger Nano S, Nano X hoặc trong ví phần cứng](#using-a-ledger-nano-hardware-wallet).

Hướng dẫn này mô tả cách thiết lập ví bằng SolFlare, cách gửi và nhận mã thông báo SOL, cũng như cách tạo và quản lý tài khoản stake.

## Bắt đầu

Truy cập https://www.solflare.com trong một trình duyệt được hỗ trợ.  Hầu hết các trình duyệt web phổ biến sẽ hoạt động khi tương tác với Keystore File, nhưng hiện tại chỉ Chrome và Brave hỗ trợ và tương tác với Ledger Nano.

### Sử dụng Keystore File

#### Tạo Keystore File mới
Để tạo ví bằng Keystore File, hãy nhấp vào "Create a Wallet" và chọn "Using Keystore File".  Làm theo lời nhắc để tạo mật khẩu, được sử dụng để mã hóa Keystore File của bạn, sau đó để tải tệp mới xuống máy tính của bạn.  Sau đó, bạn sẽ được nhắc tải Keystore File để xác minh rằng tệp tải xuống đã được lưu chính xác.

**LƯU Ý: Nếu bạn mất Keystore File hoặc mật khẩu được sử dụng để mã hóa nó, mọi khoản tiền trong ví đó sẽ bị mất vĩnh viễn.  Cả nhóm Solana và nhà phát triển SolFlare đều không thể giúp bạn khôi phục các khóa bị mất.**

Bạn nên xem xét việc lưu bản sao lưu Keystore File của mình trên một ổ đĩa cứng ngoài tách biệt với máy tính và lưu trữ mật khẩu của bạn ở một nơi an toàn.

#### Truy cập ví của bạn bằng Keystore File
Để sử dụng SolFlare với Keystore File đã tạo trước đó, hãy nhấp vào "Access a Wallet" và chọn "Using Keystore File".  Nếu bạn vừa tạo một Keystore File mới, bạn sẽ được đưa trực tiếp đến trang Access. Bạn sẽ được nhắc nhập mật khẩu và tải lên Keystore File của mình, sau đó bạn sẽ được đưa đến trang chính của giao diện ví.

### Sử dụng ví phần cứng Ledger Nano

*LƯU Ý: Vui lòng xem [các vấn đề đã biết](ledger-live.md#known-issues) để biết bất kỳ hạn chế nào trong việc sử dụng Nano.*

#### Thiết Lập Thiết Bị Ban đầu
Để sử dụng Ledger Nano với SolFlare, trước tiên hãy đảm bảo bạn đã [Thiết lập Nano của mình](ledger-live.md) và đã [cài đặt phiên bản mới nhất của ứng dụng Solana](ledger-live.md#upgrade-to-the-latest-version-of-the-solana-app) trên thiết bị của mình.

#### Chọn một địa chỉ Ledger để truy cập
Cắm Nano của bạn và mở ứng dụng Solana để màn hình thiết bị hiển thị "Application is Ready".

Từ trang chủ SolFlare, nhấp vào "Access a Wallet", sau đó chọn "Using Ledger Nano S | Ledger Nano X".  Trong "Select derivation path", hãy chọn tùy chọn duy nhất:

``Solana - 44`/501`/``

Lưu ý: Trình duyệt của bạn có thể nhắc bạn hỏi liệu SolFlare có thể giao tiếp với thiết bị Ledger của bạn hay không.  Nhấp để cho phép điều này.

Chọn một địa chỉ để tương tác từ hộp thả xuống bên dưới, sau đó nhấp vào "Access".

Thiết bị Ledger có thể lấy được một số lượng lớn các private key và các địa chỉ công khai liên quan. Điều này cho phép bạn quản lý và tương tác với một số lượng tùy ý các tài khoản khác nhau trên cùng một thiết bị.

Nếu bạn gửi tiền vào một địa chỉ từ thiết bị Ledger của bạn, hãy đảm bảo truy cập vào cùng một địa chỉ khi sử dụng SolFlare để có thể truy cập các khoản tiền đó.  Nếu bạn kết nối với địa chỉ không chính xác, chỉ cần nhấp vào Logout và kết nối lại với địa chỉ chính xác.

## Chọn một mạng

Solana duy trì [3 mạng riêng biệt](../clusters), mỗi mạng có mục đích riêng trong việc hỗ trợ hệ sinh thái Solana.  Mainnet Beta được chọn theo mặc định trên SolFlare, vì đây là mạng cố định nơi các sàn giao dịch và các ứng dụng sản xuất khác được triển khai.  Để chọn một mạng khác, hãy nhấp vào tên của mạng hiện được chọn ở đầu bảng điều khiển ví, Mainnet, Testnet hoặc Devnet, sau đó nhấp vào tên của mạng bạn muốn sử dụng.

## Gửi và nhận mã thông báo SOL

### Nhận
Để nhận mã thông báo vào ví của bạn, ai đó phải chuyển nó đến địa chỉ ví của bạn.  Địa chỉ được hiển thị ở trên cùng bên trái trên màn hình và bạn có thể nhấp vào biểu tượng Copy để sao chép địa chỉ và cung cấp địa chỉ đó cho bất kỳ ai muốn gửi mã thông báo cho bạn.  Nếu bạn giữ mã thông báo trong một ví khác hoặc trên một sàn giao dịch, bạn cũng có thể rút tiền về địa chỉ này.  Sau khi chuyển khoản được thực hiện, số dư hiển thị trên SolFlare sẽ được cập nhật trong vòng vài giây.

### Gửi
Khi bạn có một số mã thông báo tại địa chỉ ví của mình, bạn có thể gửi chúng đến bất kỳ địa chỉ ví nào khác hoặc địa chỉ gửi tiền trao đổi bằng cách nhấp vào "Transfer SOL" ở góc trên bên phải.  Nhập vào địa chỉ người nhận và số lượng SOL cần chuyển và nhấp vào "Submit".  Bạn sẽ được nhắc xác nhận chi tiết của giao dịch trước khi bạn [sử dụng chìa khóa của mình để ký giao dịch](#signing-a-transaction) và sau đó nó sẽ được gửi đến mạng.

## Staking các mã thông báo SOL
SolFlare hỗ trợ tạo, quản lý các tài khoản và ủy quyền stake.  Để tìm hiểu về cách staking trên Solana nói chung, hãy xem [Hướng dẫn Staking](../staking) của chúng tôi.

### Tạo Tài Khoản Stake
Bạn có thể sử dụng một số mã thông báo SOL trong ví của mình để tạo một tài khoản stake mới. Từ trang chính của ví, nhấp vào "Staking" ở đầu trang.  Ở phía trên bên phải, nhấp vào "Create Account".  Nhập số lượng SOL bạn muốn sử dụng để nạp tiền vào tài khoản stake mới của mình.  Số tiền này sẽ được rút từ ví của bạn và chuyển vào tài khoản stake.  Không chuyển toàn bộ số dư ví của bạn sang tài khoản stake, vì ví vẫn sử dụng để thanh toán bất kỳ khoản phí giao dịch nào liên quan đến tài khoản stake của bạn.  Cân nhắc để lại ít nhất 1 SOL trong tài khoản ví của bạn.

Sau khi gửi và [ký giao dịch](#signing-a-transaction), bạn sẽ thấy tài khoản stake mới của mình xuất hiện trong hộp có nhãn "Your Staking Accounts".

Tài khoản stake được tạo trên SolFlare đặt địa chỉ ví của bạn làm [cơ quan Staking và rút tiền](../staking/stake-accounts#understanding-account-authorities) cho tài khoản mới của bạn, điều này cung cấp cho khóa của ví bạn quyền ký cho bất kỳ giao dịch nào liên quan đến tài khoản stake mới.

### Xem Các Tài Khoản Stake của bạn
Trên trang tổng quan chính của Ví hoặc trên trang tổng quan Staking, các tài khoản stake của bạn sẽ hiển thị trong hộp "Your Staking Accounts".  Tài khoản stake tồn tại ở một địa chỉ khác với ví của bạn.

SolFlare sẽ xác định vị trí hiển thị bất kỳ tài khoản stake nào trên [mạng đã chọn](#select-a-network) mà địa chỉ ví của bạn được chỉ định làm [cơ quan stake](../staking/stake-accounts#understanding-account-authorities). Các tài khoản stake được tạo bên ngoài SolFlare cũng sẽ được hiển thị và có thể được quản lý miễn là ví bạn đăng nhập được chỉ định làm cơ quan stake.

### Ủy quyền mã thông báo trong Tài khoản Stake
Khi bạn đã [chọn một validator](../staking#select-a-validator), bạn có thể ủy quyền mã thông báo trong tài khoản stake của mình cho họ.  Từ trang tổng quan Staking, nhấp vào "Delegate" được hiển thị ở phía bên phải của tài khoản stake. Chọn validator mà bạn muốn ủy quyền từ danh sách và nhấp vào Delegate.

Để hủy ủy quyền mã thông báo đã stake của bạn (còn được gọi là hủy kich hoạt stake của bạn), quy trình cũng tương tự.  Trên trang Staking, ở phía bên phải của tài khoản stake được ủy quyền, hãy nhấp vào nút "Undelegate" và làm theo lời nhắc.

### Chia nhỏ Tài khoản Stake
Bạn có thể chia một tài khoản stake hiện có thành hai tài khoản stake.  Nhấp vào địa chỉ của tài khoản stake do ví của bạn kiểm soát và dưới thanh hành động, hãy nhấp vào "Split".  Xác định số lượng mã thông báo SOL mà bạn muốn chia.  Đây sẽ là số lượng mã thông báo trong tài khoản stake mới của bạn và số dư tài khoản stake hiện tại của bạn sẽ bị giảm đi số tiền tương tự.  Việc chia nhỏ tài khoản stake cho phép bạn ủy quyền cho nhiều validator khác nhau với số lượng mã thông báo khác nhau. Bạn có thể chia tài khoản stake bao nhiêu lần tùy thích, tạo ra bao nhiêu tài khoản stake tùy thích.

## Ký giao dịch
Bất kỳ khi nào bạn gửi một giao dịch, chẳng hạn như gửi mã thông báo đến một ví khác hoặc ủy quyền stake, bạn cần sử dụng private key của mình để ký giao dịch để giao dịch đó được mạng chấp nhận.

### Sử dụng Keystore File
Nếu bạn đã truy cập ví của mình bằng Keystore File, bạn sẽ được nhắc nhập mật khẩu của mình bất kỳ lúc nào khi cần khóa để ký giao dịch.

### Sử dụng Ledger Nano
Nếu bạn đã truy cập ví của mình bằng Ledger Nano, bạn sẽ được nhắc xác nhận chi tiết giao dịch đang chờ xử lý trên thiết bị của mình bất cứ khi nào cần khóa để ký. Trên Nano, sử dụng các nút trái và phải để xem và xác nhận tất cả các chi tiết giao dịch.  Nếu mọi thứ đều chính xác, hãy tiếp tục nhấp vào nút bên phải cho đến khi màn hình hiển thị "Approve".  Nhấp vào cả hai nút để chấp thuận giao dịch. Nếu có điều gì đó không chính xác, hãy nhấn nút bên phải một lần nữa để màn hình hiển thị "Reject" và nhấn cả hai nút để từ chối giao dịch.  Sau khi bạn chấp thuận hoặc từ chối một giao dịch, bạn sẽ thấy điều này được hiển thị trên trang SolFlare.
