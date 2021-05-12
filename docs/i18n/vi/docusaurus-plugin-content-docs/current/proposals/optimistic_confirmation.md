---
title: Xác nhận Lạc quan
---

## Nguyên thủy

`vote(X, S)` - Các phiếu bầu sẽ được tăng cường với một slot "tham chiếu", `X` là tổ tiên **mới nhất** của fork này mà validator này đã bỏ phiếu với một bằng chứng chuyển đổi. Miễn là validator thực hiện các phiếu bầu liên tiếp có giá trị giảm dần của nhau, thì các phiếu bầu `X` đó sẽ được sử dụng giống nhau. Khi validator bỏ phiếu cho một slot `s` không giảm dần so với trước đó, `X` sẽ được đặt thành `s` slot mới. Tất cả phiếu bầu sau đó sẽ có dạng `vote(X, S)`, trong đó `S` là danh sách các slot `(s, s.lockout)` được sắp xếp sẽ được bầu chọn.

Với một phiếu bầu `vote(X, S)`, hãy đặt `S.last == vote.last` là slot cuối cùng trong `S`.

Bây giờ chúng ta xác định một số điều kiện slashing "Chém Lạc Quan". Trực giác cho những điều này được mô tả dưới đây:

- `Intuition`:: Nếu một validator đệ trình `vote(X, S)`, thì cũng chính validator đó không nên bỏ phiếu cho một fork khác "chồng lên" fork này. Cụ thể hơn, validator này không nên bỏ phiếu biểu quyết khác `vote(X', S')` trong phạm vi đó `[X, S.last]` chồng lên phạm vi `[X, S.last]`, như được hiển thị bên dưới:

```text
                                  +-------+
                                  |       |
                        +---------+       +--------+
                        |         |       |        |
                        |         +-------+        |
                        |                          |
                        |                          |
                        |                          |
                    +---+---+                      |
                    |       |                      |
                X   |       |                      |
                    |       |                      |
                    +---+---+                      |
                        |                          |
                        |                      +---+---+
                        |                      |       |
                        |                      |       |  X'
                        |                      |       |
                        |                      +---+---+
                        |                          |
                        |                          |
                        |                          |
                        |                          |
                        |                      +---+---+
                        |                      |       |
                        |                      |       |  S'.last
                        |                      |       |
                        |                      +-------+
                        |
                    +---+---+
                    |       |
                 s  |       |
                    |       |
                    +---+---+
                        |
                        |
                        |
                        |
                    +---+---+
                    |       |
             S.last |       |
                    |       |
                    +-------+
```

(Ví dụ về phiếu bầu có thể trượt bỏ phiếu (X ', S') và phiếu bầu (X, S))

Trong sơ đồ trên, lưu ý rằng phiếu bầu cho `S.last` phải được gửi sau khi bỏ phiếu cho `S'.last` (do bị khóa, phiếu bầu cao hơn phải được gửi sau). Vì vậy, dãy số phiếu bầu phải là: `X ... S'.last ... S.last`. Điều này có nghĩa là sau khi bỏ phiếu `S'.last`, validator phải quay trở lại fork khác tại một số slot `s > S'.last > X`. Do đó, phiếu bầu cho `S.last` lẽ ra phải sử dụng `s` làm điểm "tham chiếu", chứ không phải `X`, vì đó là "công tắc" cuối cùng trên fork.

Bây giờ chúng ta xác định một số điều kiện slashing "Chém Lạc Quan". Với hai phiếu bầu khác nhau `vote(X, S)` </code> và `vote(X', S')` bởi cùng một validator, phiếu bầu phải đáp ứng:

- `X <= S.last`, `X' <= S'.last`
- Tất cả các `s` trong `S` là tổ tiên/hậu duệ của nhau, tất cả các `s'` trong `S'` là tổ tiên/hậu duệ của nhau,
-
- `X == X'` ngụ ý `S` là mẹ của `S'` hoặc `S'` là mẹ của `S`
- `X' > X` ngụ ý `X' > S.last` và `S'.last > S.last` và cho tất cả `s` trong `S`, `s + lockout(s) < X'`
- `X > X'` ngụ ý `X > S'.last` và `S.last > S'.last` và cho tất cả `s` trong `S'`, `s + lockout(s) < X`

(Hai quy tắc cuối cùng ngụ ý rằng các phạm vi không được trùng nhau): Nếu không, validator sẽ bị slashing.

`Range(vote)` - Đưa ra một phiếu bầu `v = vote(X, S)`, xác định `Range(v)` là phạm vi của các slot `[X, S.last]`.

`SP(old_vote, new_vote)` - Đây là "Switching Proof" cho `old_vote`, phiếu bầu mới nhất của validator. Một bằng chứng như vậy là cần thiết bất cứ khi nào validator chuyển đổi slot "tham chiếu" của họ (xem phần bỏ phiếu ở trên). Switching Proof bao gồm tham chiếu đến `old_vote`, để có một bản ghi về "phạm vi" của `old_vote` đó là gì (để làm cho các công tắc xung đột khác trong phạm vi này có thể sử dụng được). Một công tắc như vậy vẫn phải tôn trọng các khóa.

Switching proof cho thấy rằng `> 1/3`mạng bị khóa tại slot `old_vote.last`.

Bằng chứng là một danh sách các phần tử `(validator_id, validator_vote(X, S))`, trong đó:

1. Tổng số stake của tất cả id validator `> 1/3`

2. Đối với mỗi `(validator_id, validator_vote(X, S))`, tồn tại một số slot `s` trong `S`` đó:
_ a.<code>s` không phải là tổ tiên chung của cả `validator_vote.last` và `old_vote.last` và `new_vote.last`. _ b. `s` không phải là hậu duệ của `validator_vote.last`. \* c. `s + s.lockout() >= old_vote.last` (ngụ ý rằng validator vẫn bị khóa trên slot `s` tại slot `old_vote.last`).

Việc chuyển đổi các nhánh mà không có bằng chứng chuyển đổi hợp lệ có thể dễ dàng chuyển đổi.

## Định nghĩa:

Xác nhận Lạc quan - Một khối `B` sau đó được cho là đã đạt được "xác nhận lạc quan" nếu `>2/3` stake đã bỏ phiếu với số phiếu `v` trong đó `Range(v)` cho mỗi `v` như vậy bao gồm `B.slot`.

Đã hoàn thành - Một khối `B` được cho là đã hoàn thành nếu có ít nhất một validator đã bắt nguồn từ `B</code hoặc con của <code>B`.

Đã hoàn nguyên - Một khối `B` được cho là đã được hoàn nguyên nếu một khối khác `B'` không phải là cha mẹ hoặc con cháu của `B` đã được hoàn thành.

## Đảm bảo:

Một khối `B` đã đạt đến xác nhận lạc quan sẽ không được hoàn nguyên trừ khi có ít nhất có một validator bị slashing.

## Bằng chứng:

Giả sử vì mâu thuẫn lợi ích, một khối `B` đã đạt được `optimistic confirmation` tại một số slot `B + n` đối với `n`, và:

- Một khối khác `B'` không phải là khối cha mẹ hoặc hậu duệ của `B` đã được hoàn thiện.
- Không có các validator vi phạm bất kỳ điều kiện slashing nào.

Theo định nghĩa của `optimistic confirmation`</code>, điều này có nghĩa là `> 2/3` của các validator từng hiển thị một số phiếu bầu `v` của biểu mẫu `Vote(X, S)` ở `X <= B <= v.last`. Gọi tập hợp các validator này là `Optimistic Validators`.

Ngay bây giờ một validator cung cấp `v` trong `Optimistic Validators`, đưa ra hai phiếu bầu được thực hiện bởi `v`, `Vote(X, S)` và `Vote(X', S')` trong `X <= B <= S.last` và `X' <= B <= S'.last`, thì `X == X'` không vi phạm điều kiện "Optimistic Slashing" ("các dãy" của mối lá phiếu sẽ chồng chéo lên nhau tại `B`).

Do đó, hãy xác định `Optimistic Votes` là tập hợp các phiếu bầu được thực hiện bởi `Optimistic Validators`, nơi đối với validator lạc quan `v`, phiếu bầu được thực hiện bởi `v` có trong tập hợp là `maximal` phiếu bầu `Vote(X, S)` với `S.last` lớn nhất trong số các phiếu bầu của `v` đáp ứng `X <= B <= S.last`. Bởi vì chúng tôi biết từ trên `X` cho tất cả các phiếu như vậy được thực hiện bởi `v` là duy nhất, chúng tôi biết có một phiếu bầu `maximal` duy nhất như vậy.

### Bổ đề 1:

`Claim:` Đưa ra một phiếu bầu `Vote(X, S)` được thực hiện bởi một validator `V` trong `Optimistic Validators` và `S` chứa phiếu bầu cho một slot `s` mà:

- `s + s.lockout > B`,
- `s` không phải là tổ tiên hoặc hậu duệ của `B`,

sau đó `X > B`.

```text
                                  +-------+
                                  |       |
                        +---------+       +--------+
                        |         |       |        |
                        |         +-------+        |
                        |                          |
                        |                          |
                        |                          |
                        |                      +---+---+
                        |                      |       |
                        |                      |       |  X'
                        |                      |       |
                        |                      +---+---+
                        |                          |
                        |                          |
                        |                      +---+---+
                        |                      |       |
                        |                      |       |  B (Optimistically Confirmed)
                        |                      |       |
                        |                      +---+---+
                        |                          |
                        |                          |
                        |                          |
                        |                      +---+---+
                        |                      |       |
                        |                      |       |  S'.last
                        |                      |       |
                        |                      +-------+
                        |
                    +---+---+
                    |       |
                 X  |       |
                    |       |
                    +---+---+
                        |
                        |
                        |
                        |
                        |
                        |
                    +---+---+
                    |       |
            S.last  |       |
                    |       |
                    +---+---+
                        |
                        |
                        |
                        |
                    +---+---+
                    |       |
      s + s.lockout |       |
                    +-------+
```

`Proof`: Giả sử vì mâu thuẫn lợi ích với một validator `V` từ "Optimistic Validators" đã thực hiện một phiếu bầu như vậy `Vote(X, S)` trong đó `S` có phiếu bầu cho một slot `s` không phải là tổ tiên hoặc hậu duệ của `B`, ở `s + s.lockout > B`, nhưng `X <= B`.

Hãy để `Vote(X', S')` là phiếu bầu `Optimistic Votes` được thực hiện bởi validator `V`. Theo định nghĩa của tập hợp đó (tất cả các phiếu bầu được xác nhận một cách lạc quan `B`), `X' <= B <= S'.last` (xem sơ đồ ở trên).

Điều này ngụ ý rằng vì nó được giả định ở trên `X <= B`, sau đó là `X <= S'.last`, vì vậy theo quy tắc slashing, `X == X'` hoặc `X < X'` (nếu không sẽ chồng chéo lên `(X', S'.last)`).

`Case X == X'`:

Hãy xem xét `s`. Chúng ta biết `s != X` bởi vì nó được giả định rằng `s` không phải là tổ tiên hoặc hậu duệ của `B` và `X` là tổ tiên của `B`. Vì `S'.last` là hậu duệ của `B`, điều này có nghĩa là `s` cũng không phải là tổ tiên hoặc hậu duệ của `S'.last`. Sau đó, bởi vì `S.last` là hậu duệ của `s`, nên `S'.last` cũng không thể là tổ tiên hoặc hậu duệ của `S.last`. Điều này ngụ ý `X != X'` theo quy tắc "Optimistic Slashing".

`Case X < X'`:

Về mặt trực quan, điều này ngụ ý rằng `Vote(X, S)` được tạo ra "trước đây" `Vote(X', S')`.

Từ giả thuyết trên, `s + s.lockout > B > X'`. Bởi vì `s` không phải là tổ tiên của `X'`, các khóa sẽ bị vi phạm khi validator này lần đầu tiên cố gắng gửi một phiếu bầu chuyển đổi sang `X'` với một số phiếu bầu của biểu mẫu `Vote(X', S'')`.

Vì không có trường hợp nào trong số này là hợp lệ, nên giả định phải không hợp lệ, và yêu cầu được chứng minh.

### Bổ đề 2:

Nhớ lại `B'` là khối được hoàn thiện trên một fork khác với khối "được xác nhận một cách lạc quan" `B`.

`Claim`: Đối với bất kỳ phiếu bầu nào `Vote(X, S)` trong tập hợp `Optimistic Votes`, nó phải đúng rằng `B' > X`

```text
                                +-------+
                                |       |
                       +--------+       +---------+
                       |        |       |         |
                       |        +-------+         |
                       |                          |
                       |                          |
                       |                          |
                       |                      +---+---+
                       |                      |       |
                       |                      |       |  X
                       |                      |       |
                       |                      +---+---+
                       |                          |
                       |                          |
                       |                      +---+---+
                       |                      |       |
                       |                      |       |  B (Optimistically Confirmed)
                       |                      |       |
                       |                      +---+---+
                       |                          |
                       |                          |
                       |                          |
                       |                      +---+---+
                       |                      |       |
                       |                      |       |  S.last
                       |                      |       |
                       |                      +-------+
                       |
                   +---+---+
                   |       |
    B'(Finalized)  |       |
                   |       |
                   +-------+
```

`Proof`: Hãy để `Vote(X, S)` là một phiếu bầu trong tập hợp `Optimistic Votes`. Sau đó, theo định nghĩa, đưa ra khối "được xác nhận một cách tối ưu" `B`, `X <= B <= S.last`.

Vì `X` là mẹ của `B` và `B'` không phải là mẹ hoặc tổ tiên của `B`, thì:

- `B' != X`
- `B'` không phải là cha mẹ của `B'`

Bây giờ hãy xem xét nếu `B'` < `X`:

`Case B' < X`: Chúng tôi nhận thấy đây là một hành vi vi phạm khóa. Từ trên, chúng ta biết `B'` không phải là cha mẹ của `X`. Bởi vì `B'` đã được root, và `B'` không phải là cha mẹ của `X`, nên validator sẽ không thể bỏ phiếu trên slot cao hơn `X` không giảm xuống từ `B'`.

### Bằng Chứng An Toàn:

Bây giờ chúng tôi muốn hiển thị ít nhất một trong các validator trong `Optimistic Validators` vi phạm quy tắc slashing.

Trước tiên, lưu ý rằng để `B'` được root, phải có `> 2/3` stake đã bỏ phiếu trên `B'` hoặc hậu duệ của `B'`. Giả sử rằng `Optimistic Validator` tập hợp này cũng chứa `> 2/3` của các validator đã stake, nó tuân theo tập hợp `> 1/3` của các validator stake:

- Có root `B'` hoặc hậu duệ của `B'`
- Đồng thời gửi một phiếu bầu `v` của biểu mẫu `v` ở `v`.

Đặt `Delinquent` là tập hợp các validator đáp ứng các tiêu chí trên.

Theo định nghĩa, để root `B'`, mỗi validator `V` trong `Delinquent` Mỗi người phải thực hiện một số "chuyển đổi phiếu bầu" của biểu mẫu `Vote(X_v, S_v)` trong đó:

- `S_v.last > B'`
- `S_v.last` là hậu duệ của `B'`, vì vậy nó không thể là hậu duệ của `B`
- Vì `S_v.last` không phải là hậu duệ của `B`, vậy thì `X_v` không thể là hậu duệ hoặc tổ tiên của `B`.

Theo định nghĩa, validator quá hạn `V` này cũng đã thực hiện một số phiếu bầu `Vote(X, S)` về `Optimistic Votes` nơi theo định nghĩa của tập hợp đó ( được xác nhận một cách lạc quan `B`), chúng tôi biết `S.last >= B >= X`.

Theo `Lemma 2` chúng ta biết `B' > X`, và từ trên cao `S_v.last > B'`, vì vậy sau đó `S_v.last > X`. Vì `X_v != X` (không thể là hậu duệ hoặc tổ tiên của `B` từ trên xuống), nên theo quy tắc slashing, thì ta biết `X_v > S.last`. Từ phía trên, `S.last >= B >= X`, đối với tất cả các " chuyển đổi phiếu bầu" như vậy, `X_v > B`.

Bây giờ đặt hàng tất cả những "chuyển đổi phiếu bầu" trong thời gian này, hãy để `V` là validator trong `Optimistic Validators` lần đầu tiên gửi một "chuyển đổi phiếu bầu" như vậy `Vote(X', S')`, ở `X' > B`. Chúng tôi biết rằng validator như vậy tồn tại bởi vì chúng tôi biết từ phía trên rằng tất cả các validator quá hạn phải gửi một phiếu bầu như vậy và những validator quá hạn là một tập hợp con của `Optimistic Validators`.

Hãy để `Vote(X, S)` là phiếu bầu duy nhất trong `Optimistic Votes` được thực hiện bởi validator `V` (tối đa hóa `S.last`).

Do `Vote(X, S)` vì `X' > B >= X`, sau đó `X' > X`, vì vậy theo quy tắc "Optimistic Slashing", `X' > S.last`.

Để thực hiện "chuyển đổi phiếu bầu" thành `X'``, một switching proof
<code>SP(Vote(X, S), Vote(X', S'))` phải cho thấy `> 1/3` stake bị khóa tại cuộc bỏ phiếu mới nhất của validator này, `S.last`. Kết hợp `>1/3` này với thực tế là các thiết lập của các validator trong `Optimistic Voters` bao gồm `> 2/3` của stake, ngụ ý ít nhất một validator lạc quan `W` từ `Optimistic Voters` phải gửi một phiếu bầu (nhớ lại định nghĩa của switching proof), `Vote(X_w, S_w)` đã được bao gồm trong validator `V` chuyển đổi bằng chứng cho slot `X'`, ở `S_w` chứa một slot `s` như vậy:

- `s` không phải là tổ tiên chung của `S.last` và `X'`
- `s` không phải là hậu duệ của `S.last`.
- `s' + s'.lockout > S.last`

Bởi vì `B` là tổ tiên của `S.last` nên nó cũng đúng sau đó:

- `s` không phải là tổ tiên chung của `B` và `X'`
- `s' + s'.lockout > B`

đã được bao gồm trong switching proof của `V`.

Bây giờ vì `W` cũng là một thành viên của `Optimistic Voters`, sau đó theo `Lemma 1` ở trên, đưa ra một phiếu bầu bởi `W`, `Vote(X_w, S_w)` ở `S_w`` chứa một phiếu bầu cho
một slot <code>s` ở `s + s.lockout > B` và `s` không phải là tổ tiên của `B`, sau đó `X_w > B`.

Bởi vì validator `V` đã bao gồm bỏ phiếu `Vote(X_w, S_w)` trong proof of switching của nó cho slot `X'`, thì validator ngụ ý rằng `V'` đã gửi bỏ phiếu `Vote(X_w, S_w)` **trước khi** validator `V` gửi phiếu bầu chuyển đổi cho slot `X'`, `Vote(X', S')`.

Nhưng đây là một mâu thuẫn bởi vì chúng tôi đã chọn `Vote(X', S')` là phiếu bầu đầu tiên được thực hiện bởi bất kỳ validator nào trong `Optimistic Voters` đặt ở `X' > B` và `X'` không phải là hậu duệ của `B`.
