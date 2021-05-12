---
title: Lập trình Stake
---

Để tối đa hóa việc phân phối stake, phân quyền và khả năng chống kiểm duyệt trên mạng Solana, việc stakingcó thể được thực hiện theo chương trình. Nhóm và cộng đồng đã phát triển một số chương trình trên chuỗi và ngoài chuỗi để giúp quản lý việc stake dễ dàng hơn.

#### Stake-o-matic hay còn gọi là Bot tự động ủy quyền
Chương trình off-chain này quản lý một lượng lớn các validator do một cơ quan trung ương tham gia stake. Solana Foundation sử dụng một bot tự động ủy quyền để thường xuyên ủy thác stake của mình cho những validator "không quá hạn" đáp ứng các yêu cầu về hiệu suất được chỉ định. Thông tin thêm có thể được tìm thấy trên [thông báo chính thức](https://forums.solana.com/t/stake-o-matic-delegation-matching-program/790).

#### Stake Pools
Chương trình pool này gộp SOL lại với nhau do người quản lý stake, cho phép người sở hữu SOL tham gia stake và kiếm phần thưởng mà không cần quản lý việc stake. Người dùng gửi SOL để đổi lấy mã thông báo SPL (staking phái sinh) đại diện cho quyền sở hữu của họ trong stake pool. Người quản lý pool stake SOL theo chiến lược của họ, có thể sử dụng một biến thể của bot tự động ủy quyền như được mô tả ở trên. Khi tiền stake kiếm được phần thưởng, pool và mã thông báo pool tăng tương ứng về giá trị. Cuối cùng, chủ sở hữu mã thông báo pool có thể gửi mã thông báo SPL trở lại pool stake để đổi SOL, do đó tham gia vào phân quyền với ít công việc hơn yêu cầu. Thông tin thêm có thể được tìm thấy tại [Tài liệu về stake pool SPL](https://spl.solana.com/stake-pool).
