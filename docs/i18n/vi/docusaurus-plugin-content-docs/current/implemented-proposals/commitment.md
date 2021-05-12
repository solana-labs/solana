---
title: Lời cam kết
---

Chỉ số cam kết nhằm cung cấp cho khách hàng một thước đo về xác nhận mạng và mức stake trên một khối cụ thể. Sau đó, khách hàng có thể sử dụng thông tin này để đưa ra các biện pháp cam kết của riêng họ.

# Tính toán RPC

Khách hàng có thể yêu cầu số liệu cam kết từ validator cho chữ ký `s` thông qua `get_block_commitment(s: Signature) -> BlockCommitment` qua RPC. Cấu trúc `BlockCommitment` chứa một mảng u64 `[u64, MAX_CONFIRMATIONS]`. Mảng này đại diện cho số liệu cam kết cho khối cụ thể `N` có chứa chữ ký `s` kể từ khối cuối cùng `M` mà validator đã bỏ phiếu.

Mục nhập `s` tại chỉ mục `i` trong mảng `BlockCommitment` ngụ ý rằng validator đã quan sát thấy tổng số stake `s` trong cụm đạt được `i` xác nhận trên khối `N` như được quan sát trong một số khối `M`. Sẽ có các phần tử `MAX_CONFIRMATIONS` trong mảng này, đại diện cho tất cả số lượng xác nhận có thể có từ 1 đến `MAX_CONFIRMATIONS`.

# Tính toán số liệu cam kết

Việc xây dựng cấu trúc `BlockCommitment` này tận dụng các tính toán đã được thực hiện để xây dựng sự đồng thuận. Hàm `collect_vote_lockouts` trong `consensus.rs` xây dựng một HashMap, trong đó mỗi mục nhập có dạng `(b, s)` trong đó `s` là số tiền stake của ngân hàng `b`.

Việc tính toán này được thực hiện trên một ngân hàng ứng viên có thể thay đổi được `b` như sau.

```text
   let output: HashMap<b, Stake> = HashMap::new();
   for vote_account in b.vote_accounts {
       for v in vote_account.vote_stack {
           for a in ancestors(v) {
               f(*output.get_mut(a), vote_account, v);
           }
       }
   }
```

trong đó `f` là một số hàm tích lũy sửa đổi mục nhập `Stake` cho slot `a` với một số dữ liệu có được từ phiếu bầu `v` và `vote_account` (stake, lockout, v. v.). Lưu ý ở đây rằng `ancestors` ở đây chỉ bao gồm các slot hiện diện trong bộ đệm trạng thái hiện tại. Dù sao thì các chữ ký cho các ngân hàng sớm hơn các chữ ký có trong bộ đệm trạng thái sẽ không thể truy vấn được, do đó các ngân hàng đó không được đưa vào các tính toán cam kết ở đây.

Giờ đây, chúng ta có thể tăng cường tính toán trên một cách tự nhiên để xây dựng một mảng `BlockCommitment` cho mọi ngân hàng `b` bằng cách:

1. Thêm một `ForkCommitmentCache` để thu thập các cấu trúc `BlockCommitment`
2. Thay thế `f` bằng `f'` để tính toán trên cũng xây dựng `BlockCommitment` cho mọi ngân hàng `b`.

Chúng tôi sẽ tiếp tục với các chi tiết của 2) như 1) là không đáng kể.

Trước khi tiếp tục, cần lưu ý rằng đối với một số tài khoản bỏ phiếu của validator `a`, số lượng xác nhận cục bộ cho validator đó trên slot `s` là `v.num_confirmations`, trong đó `v` là phiếu bầu nhỏ nhất trong chồng phiếu bầu `a.votes` sao cho `v.slot >= s` (tức là không cần xem phiếu bầu > v vì số lượng xác nhận sẽ thấp hơn).

Bây giờ cụ thể hơn, chúng tôi tăng cường tính toán trên thành:

```text
   let output: HashMap<b, Stake> = HashMap::new();
   let fork_commitment_cache = ForkCommitmentCache::default();
   for vote_account in b.vote_accounts {
       // vote stack is sorted from oldest vote to newest vote
       for (v1, v2) in vote_account.vote_stack.windows(2) {
           for a in ancestors(v1).difference(ancestors(v2)) {
               f'(*output.get_mut(a), *fork_commitment_cache.get_mut(a), vote_account, v);
           }
       }
   }
```

trong đó `f'` được định nghĩa là:

```text
    fn f`(
        stake: &mut Stake,
        some_ancestor: &mut BlockCommitment,
        vote_account: VoteAccount,
        v: Vote, total_stake: u64
    ){
        f(stake, vote_account, v);
        *some_ancestor.commitment[v.num_confirmations] += vote_account.stake;
    }
```
