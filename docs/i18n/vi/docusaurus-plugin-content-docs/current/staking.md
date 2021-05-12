---
title: Staking trên Solana
---

_Lưu ý trước khi đọc: Tất cả các tham chiếu đến sự gia tăng giá trị là tuyệt đối liên quan đến sự cân bằng của SOL. Tài liệu này không đưa ra gợi ý về giá trị tiền tệ của SOL tại bất kỳ thời điểm nào._

Staking mã thông báo SOL của bạn trên Solana là cách tốt nhất bạn có thể giúp bảo mật mạng blockchain hoạt động cao nhất, và [earn rewards](implemented-proposals/staking-rewards.md) khi làm như vậy!

Solana là mạng Proof-of-Stake (PoS) với các ủy quyền, có nghĩa là chủ sở hữu nắm giữ mã thông báo SOL có thể ủy quyền SOL của họ cho một hoặc nhiều validator, người xử lý giao dịch và điều hành mạng.

Ủy quyền stake là một mô hình tài chính chia sẻ phần thưởng rủi ro có thể mang lại lợi nhuận cho những người nắm giữ mã thông báo đã ủy quyền trong một thời gian dài. Điều này đạt được bằng cách điều chỉnh các ưu đãi tài chính của người nắm giữ mã thông báo (delegators) và các validator mà họ ủy quyền.

Một validator càng được ủy quyền nhiều stake, thì validator này càng được chọn để ghi các giao dịch mới vào sổ cái thường xuyên hơn. Validator càng viết được nhiều giao dịch, họ và người ủy quyền của họ càng kiếm được nhiều phần thưởng. Các validator nâng cấp hệ thống của họ để có thể xử lý được nhiều giao dịch hơn trong cùng một lúc và còn giúp họ kiếm được nhiều phần thưởng hơn khi làm như vậy, ngoài ra họ còn giữ cho mạng hoạt động nhanh và ổn định.

Các validator phải chịu chi phí bằng cách vận hành và bảo trì hệ thống của họ và điều này được chuyển cho người ủy quyền dưới hình thức thu phí theo phần trăm phần thưởng kiếm được. Phí này được gọi là _commission_. Khi các validator kiếm được nhiều phần thưởng hơn thì số lượng ủy thác stake được giao cho họ càng nhiều hơn, và họ có thể cạnh tranh với nhau để đưa ra mức hoa hồng thấp nhất cho các dịch vụ của họ, nhằm thu hút nhiều ủy quyền stake hơn.

Có nguy cơ mất mã thông báo khi staking, thông qua một quá trình được gọi là _slashing_. Slashing involves the removal and destruction of a portion of a validator's delegated stake in response to intentional malicious behavior, such as creating invalid transactions or censoring certain types of transactions or network participants.

Nếu validator bị slashing, thì những người đã ủy quyền stake cho validator đó sẽ mất lợi nhuận của họ. Có nghĩa là sẽ tổn thất ngay lập tức cho người đã ủy quyền cho họ, và cũng là mất phần thưởng trong tương lai cho validator đó do số lượng người ủy quyền stake cho họ bị giảm sút. More details on the slashing roadmap can be found [here](proposals/optimistic-confirmation-and-slashing.md#slashing-roadmap).

Đó là tiêu chí của phần thưởng mạng và slashing để để điều chỉnh các ưu đãi tài chính của cả validator và người nắm giữ mã thông báo, từ đó giúp giữ cho mạng an toàn, mạnh mẽ và hoạt động tốt nhất.

## Làm cách nào để stake mã thông báo SOL của tôi?

Để stake mã thông báo trên Solana, trước tiên bạn sẽ cần chuyển một số SOL vào ví hỗ trợ staking, sau đó làm theo các bước hướng dẫn được cung cấp để tạo tài khoản stake và ủy quyền stake của bạn. Các ví khác nhau sẽ có các sự thay đổi khác nhau, nhưng hướng dẫn chung thì nằm ở dưới.

#### Ví được hỗ trợ

Các hoạt động staking được hỗ trợ bởi các ví sau:

- SolFlare.com kết hợp với tệp Keystore hoặc Ledger Nano. Hãy xem [guide to using SolFlare](wallet-guide/solflare.md) chúng tôi để biết thêm chi tiết.

- Các công cụ dòng lệnh Solana có thể thực hiện tất cả các hoạt động stake cùng với ví file keypair do CLI tạo, ví giấy hoặc với Ledger Nano. [Staking commands using the Solana Command Line Tools](cli/delegate-stake.md).

- [Exodus](https://www.exodus.com/) wallet. They make the process very simple, but you cannot choose a validator: they assign you to their partner validator. See their [FAQ](https://support.exodus.com/article/1551-solana-staking-faq) for details.

- [Binance](https://www.binance.com/) and [FTX](https://ftx.com/) exchanges. Note that you cannot choose a validator with these services: they assign you to their partner validator.

#### Tạo Tài Khoản Stake

Tài khoản stake là tài khoản khác với ví được sử dụng để gửi và nhận mã thông báo SOL. Nếu bạn đã nhận được SOL trong ví, bạn có thể sử dụng mã thông báo này để tạo và nạp tiền vào tài khoản stake mới, tài khoản này sẽ có địa chỉ khác với ví bạn đã sử dụng để tạo. Tùy thuộc vào ví bạn đang sử dụng, các bước để tạo tài khoản stake có thể khác nhau một chút. Không phải tất cả các ví đều hỗ trợ tài khoản stake, hãy xem [Supported Wallets](#supported-wallets).

#### Chọn Validator

Sau khi tạo tài khoản stake bạn có thể ủy quyền SOL cho một validator. Dưới đây là nơi để bạn có thể kiểm tra thông tin về những validator hiện đang tham gia điều hành mạng. Solana Labs và Solana Foundation không đề xuất bất kỳ một validator cụ thể nào.

Các validator Mainnet Beta giới thiệu bản thân và dịch vụ của họ trên Diễn đàn Solana này:

- https://forums.solana.com/t/validator-information-thread

Trang web solanabeach.io được xây dựng và duy trì bởi một trong những validator của chúng tôi, đó là Staking Facilities. Nó cung cấp một số thông tin đồ họa cao cấp về mạng nói chung, cũng như danh sách của các validator và kèm theo một số thống kê hiệu suất gần đây của từng validator.

- https://solanabeach.io

Để xem thống kê sản xuất khối, hãy sử dụng các công cụ dòng lệnh Solana:

- `validator solana`
- `sản xuất khối solana`

Nhóm Solana không đưa ra khuyến nghị về cách giải thích thông tin này. Những người ủy quyền tiềm năng nên tự mình thẩm định.

#### Ủy quyền Stake của bạn

Khi bạn đã quyết định bạn sẽ ủy quyền cho validator hoặc validator nào khác, hãy sử dụng ví được hỗ trợ để ủy quyền stake của bạn cho địa chỉ tài khoản của validator.

## Chi tiết Tài khoản Stake

Để biết thêm thông tin về các hoạt động và những những gì liên quan đến tài khoản stake, vui lòng xem [Stake Accounts](staking/stake-accounts.md)
