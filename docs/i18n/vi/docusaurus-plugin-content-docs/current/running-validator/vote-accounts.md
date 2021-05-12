---
title: Quản lý tài khoản bỏ phiếu
---

Trang này mô tả cách thiết lập _tài khoản bỏ phiếu_ trên chuỗi. Việc tạo tài khoản bỏ phiếu là cần thiết nếu bạn định chạy một validator node trên Solana.

## Tạo một tài khoản bỏ phiếu

Một tài khoản biểu quyết có thể được tạo bằng lệnh [create-vote-account](../cli/usage.md#solana-create-vote-account). Tài khoản bỏ phiếu có thể được định cấu hình khi được tạo lần đầu tiên hoặc sau khi validator đang chạy. Tất cả các khía cạnh của tài khoản bỏ phiếu có thể được thay đổi ngoại trừ [địa chỉ tài khoản bỏ phiếu](#vote-account-address), địa chỉ này được cố định trong suốt thời gian tồn tại của tài khoản.

### Định cấu hình một Tài khoản Bỏ phiếu Hiện tại

- Để thay đổi [danh tính validator](#validator-identity), hãy sử dụng [vote-update-validator](../cli/usage.md#solana-vote-update-validator).
- Để thay đổi [quyền bỏ phiếu](#vote-authority), hãy sử dụng [vote-authorize-voter](../cli/usage.md#solana-vote-authorize-voter).
- Để thay đổi [quyền rút tiền](#withdraw-authority), hãy sử dụng [vote-authorize-withdrawer](../cli/usage.md#solana-vote-authorize-withdrawer).
- Để thay đổi [hoa hồng](#commission), hãy sử dụng [vote-update-commission](../cli/usage.md#solana-vote-update-commission).

## Cấu trúc tài khoản bỏ phiếu

### Địa chỉ tài khoản bỏ phiếu

Tài khoản phiếu bầu được tạo tại một địa chỉ là public key của tệp keypair hoặc tại địa chỉ dẫn xuất dựa trên public key của tệp keypair và chuỗi hạt giống.

Địa chỉ của tài khoản bỏ phiếu không bao giờ cần thiết để ký kết bất kỳ giao dịch nào mà chỉ được sử dụng để tra cứu thông tin tài khoản.

Khi ai đó muốn [ủy quyền mã thông báo trong một tài khoản stake](../staking.md), lệnh ủy quyền được chỉ vào địa chỉ tài khoản bỏ phiếu của validator mà chủ sở hữu mã thông báo muốn ủy quyền.

### Danh tính validator

_Danh tính validator_ là một tài khoản hệ thống được sử dụng để thanh toán tất cả các khoản phí giao dịch bỏ phiếu được gửi đến tài khoản bỏ phiếu. Bởi vì validator dự kiến ​​sẽ bỏ phiếu trên hầu hết các khối hợp lệ mà nó nhận được, tài khoản danh tính của validator thường xuyên (có khả năng nhiều lần mỗi giây) ký giao dịch và trả phí. Vì lý do này, keypair danh tính validator phải được lưu trữ dưới dạng "ví nóng" trong tệp keypair trên cùng hệ thống mà validator đang chạy.

Bởi vì ví nóng thường kém an toàn hơn ví ngoại tuyến hoặc ví "lạnh", nhà điều hành validator có thể chọn chỉ lưu trữ đủ SOL trên tài khoản danh tính để trang trải phí bỏ phiếu trong một khoảng thời gian giới hạn, chẳng hạn như vài tuần hoặc vài tháng. Tài khoản danh tính của validator có thể được nạp định kỳ từ một ví an toàn hơn.

Phương pháp này có thể làm giảm nguy cơ mất tiền nếu ổ đĩa hoặc hệ thống tệp của validator node bị xâm nhập hoặc bị hỏng.

Cần phải cung cấp danh tính validator khi tạo tài khoản bỏ phiếu. Danh tính validator cũng có thể được thay đổi sau khi tạo tài khoản bằng cách sử dụng lệnh [vote-update-validator](../cli/usage.md#solana-vote-update-validator).

### Cơ quan bỏ phiếu

Keypair của _cơ quan bỏ phiếu_ được sử dụng để ký vào mỗi giao dịch bỏ phiếu mà validator node muốn gửi đến cụm. Điều này không nhất thiết phải là duy nhất từ ​​danh tính validator, như bạn sẽ thấy ở phần sau của tài liệu này. Bởi vì cơ quan bỏ phiếu, giống như danh tính validator, đang ký các giao dịch thường xuyên, nên đây cũng phải là keypair nóng trên cùng hệ thống tệp với quy trình validator.

Cơ quan bỏ phiếu có thể được đặt thành địa chỉ giống như danh tính validator. Nếu danh tính validator cũng là cơ quan bỏ phiếu, chỉ cần một chữ ký cho mỗi giao dịch biểu quyết để vừa ký vào biểu quyết vừa thanh toán phí giao dịch. Vì phí giao dịch trên Solana được đánh giá cho mỗi chữ ký, việc có một người ký thay vì hai người sẽ dẫn đến một nửa phí giao dịch được trả so với việc đặt cơ quan bỏ phiếu và danh tính validator cho hai tài khoản khác nhau.

Cơ quan bỏ phiếu có thể được đặt khi tài khoản bỏ phiếu được tạo. Nếu nó không được cung cấp, hành vi mặc định là gán nó giống như danh tính validator. Cơ quan bỏ phiếu có thể được thay đổi sau đó bằng lệnh [vote-authorize-voter](../cli/usage.md#solana-vote-authorize-voter).

Cơ quan bỏ phiếu có thể được thay đổi nhiều nhất một lần tại mỗi kỷ nguyên. Nếu cơ quan được thay đổi bằng [vote-authorize-voter](../cli/usage.md#solana-vote-authorize-voter), điều này sẽ không có hiệu lực cho đến đầu kỷ nguyên tiếp theo. Để hỗ trợ quá trình ký biểu quyết diễn ra suôn sẻ, hãy solana-`solana-validator` cho phép đối số `--authorized-voter` được chỉ định nhiều lần. Điều này cho phép quy trình validator tiếp tục bỏ phiếu thành công khi mạng đạt đến ranh giới kỷ nguyên mà tại đó tài khoản của cơ quan bỏ phiếu validator thay đổi.

### Cơ quan rút tiền

Keypair _quyền rút tiền_ được sử dụng để rút tiền từ tài khoản bỏ phiếu bằng lệnh [withdraw-from-vote-account](../cli/usage.md#solana-withdraw-from-vote-account). Bất kỳ phần thưởng mạng nào mà validator kiếm được đều được gửi vào tài khoản bỏ phiếu và chỉ có thể lấy được bằng cách ký với keypair của cơ quan rút tiền.

Cơ quan rút tiền cũng được yêu cầu ký vào bất kỳ giao dịch nào để thay đổi [hoa hồng](#commission) của tài khoản bỏ phiếu và thay đổi danh tính validator trên tài khoản bỏ phiếu.

Vì tài khoản bỏ phiếu có thể tích lũy số dư đáng kể, hãy cân nhắc giữ keypair cơ quan rút tiền trong ví ngoại tuyến/ví lạnh vì không cần thiết phải ký các giao dịch thường xuyên.

Cơ quan rút tiền có thể được thiết lập khi tạo tài khoản bỏ phiếu với tùy chọn `--authorized-withdrawer`. Nếu điều này không được cung cấp, danh tính validator sẽ được đặt làm cơ quan rút tiền theo mặc định.

Quyền rút tiền có thể được thay đổi sau đó bằng lệnh [người rút tiền bỏ phiếu](../cli/usage.md#solana-vote-authorize-withdrawer).

### Hoa hồng

_Hoa hồng_ là phần trăm phần thưởng mạng mà validator kiếm được, được gửi vào tài khoản bỏ phiếu của validator. Phần còn lại của phần thưởng được phân phối cho tất cả các tài khoản stake được ủy quyền cho tài khoản biểu quyết đó, tỷ lệ thuận với tỉ trọng tiền stake đang hoạt động của mỗi tài khoản stake.

Ví dụ, nếu một tài khoản biểu quyết có hoa hồng là 10%, đối với tất cả các phần thưởng mà validator đó kiếm được trong một kỷ nguyên nhất định, 10% trong số các phần thưởng này sẽ được gửi vào tài khoản biểu quyết trong khối đầu tiên của kỷ nguyên sau. 90% còn lại sẽ được gửi vào tài khoản stake được ủy quyền như stake hoạt động ngay lập tức.

Một validator có thể chọn đặt mức hoa hồng thấp để cố gắng thu hút nhiều ủy quyền stake hơn vì mức hoa hồng thấp hơn dẫn đến tỷ lệ phần thưởng lớn hơn được chuyển cho người đã ủy quyền. Vì có các chi phí liên quan đến việc thiết lập và vận hành một validator node, một validator lý tưởng sẽ đặt một khoản hoa hồng đủ cao để trang trải chi phí của họ.

Hoa hồng có thể được đặt khi tạo tài khoản bỏ phiếu với tùy chọn `--commission`. Nếu nó không được cung cấp, nó sẽ mặc định là 100%, điều này sẽ dẫn đến tất cả phần thưởng được gửi vào tài khoản bỏ phiếu và không có phần thưởng nào được chuyển cho bất kỳ tài khoản stake được ủy quyền nào.

Hoa hồng cũng có thể được thay đổi sau đó bằng lệnh[vote-update-commission](../cli/usage.md#solana-vote-update-commission).

Khi cài đặt hoa hồng, chỉ các giá trị nguyên trong tập hợp [0-100] mới được chấp nhận. Số nguyên đại diện cho số phần trăm cho hoa hồng, do đó tạo tài khoản với `--commission 10` sẽ đặt hoa hồng là 10%.

## Xoay chìa khóa

Việc xoay các khóa cơ quan tài khoản biểu quyết yêu cầu xử lý đặc biệt khi giao dịch với một validator trực tiếp.

### Bỏ phiếu danh tính tài khoản validator

Bạn sẽ cần quyền truy cập vào keypair _cơ quan rút tiền_ cho tài khoản bỏ phiếu để thay đổi danh tính validator. Các bước tiếp theo giả định rằng `~/withdraw-authority.json` là keypair,.

1. Tạo keypair danh tính validator mới `solana-keygen new -o ~/new-validator-keypair.json`.
2. Đảm bảo rằng tài khoản danh tính mới đã được cấp tiền `solana transfer ~/new-validator-keypair.json 500`.
3. Chạy `solana vote-update-validator ~/vote-account-keypair.json ~/new-validator-keypair.json ~/withdraw-authority.json` để sửa đổi danh tính validator trong tài khoản bỏ phiếu của bạn
4. Khởi động lại validator của bạn với keypair danh tính mới cho đối số `--identity`

### Bỏ phiếu cho Tài khoản Người được ủy quyền

Keypair của _cơ quan bỏ phiếu_ chỉ có thể được thay đổi ở ranh giới kỷ nguyên và yêu cầu một số đối số bổ sung để `solana-validator` di chuyển liền mạch.

1. Chạy `solana epoch-info`. Nếu thời gian còn lại trong kỷ nguyên hiện tại không còn nhiều, hãy cân nhắc đợi kỷ nguyên tiếp theo để cho phép validator của bạn có nhiều thời gian khởi động lại và bắt kịp.
2. Tạo keypair cơ quan bỏ phiếu mới `solana-keygen new -o ~/new-vote-authority.json`.
3. Determine the current _vote authority_ keypair by running `solana vote-account ~/vote-account-keypair.json`. Nó có thể là tài khoản danh tính của validator (mặc định) hoặc một số keypair khác. Các bước sau giả định rằng `~/validator-keypair.json` là keypair.
4. Chạy `solana vote-authorize-voter ~/vote-account-keypair.json ~/validator-keypair.json ~/new-vote-authority.json`. Cơ quan bỏ phiếu mới dự kiến ​​sẽ hoạt động bắt đầu từ kỷ nguyên tiếp theo.
5. `solana-validator` bây giờ cần được khởi động lại với keypair cơ quan bỏ phiếu cũ và mới, để nó có thể chuyển đổi suôn sẻ vào kỷ nguyên tiếp theo. Add the two arguments on restart: `--authorized-voter ~/validator-keypair.json --authorized-voter ~/new-vote-authority.json`
6. Sau khi cụm đạt đến kỷ nguyên tiếp theo, hãy loại bỏ đối số `--authorized-voter ~/validator-keypair.json` và khởi động lại `solana-validator`, vì keypair của cơ quan bỏ phiếu cũ không còn được yêu cầu nữa.

### Bỏ phiếu cho người rút tiền được ủy quyền của tài khoản

Không cần xử lý đặc biệt. Sử dụng lệnh `solana vote-authorize-withdrawer` khi cần thiết.
