---
title: Sao chép sổ cái
---

Lưu ý: giải pháp sao chép sổ cái này đã được triển khai một phần, nhưng chưa hoàn thành. Việc triển khai một phần đã bị https://github.com/solana-labs/solana/pull/9992 loại bỏ để ngăn chặn rủi ro bảo mật của code không sử dụng. Phần đầu tiên của tài liệu thiết kế này phản ánh các phần được thực hiện một lần của việc nhân rộng sổ cái. [phần thứ hai của tài liệu này](#ledger-replication-not-implemented) mô tả các phần của giải pháp chưa từng được triển khai.

## Bằng chứng nhân rộng

Ở công suất tối đa trên mạng solana 1gbps sẽ tạo ra 4 petabyte dữ liệu mỗi năm. Để ngăn mạng tập trung xung quanh các validator phải lưu trữ toàn bộ dữ liệu, giao thức này đề xuất một cách để các node khai thác cung cấp dung lượng lưu trữ cho các phần dữ liệu.

Ý tưởng cơ bản của Proof of Replication là mã hóa tập dữ liệu bằng khóa đối xứng công khai bằng cách sử dụng mã hóa CBC, sau đó hàm băm tập dữ liệu được mã hóa. Vấn đề chính của cách tiếp cận ngây thơ là một node lưu trữ không trung thực có thể phát trực tuyến mã hóa và xóa dữ liệu khi nó được băm. Giải pháp đơn giản là tạo lại hàm băm theo định kỳ dựa trên giá trị PoH đã ký. Điều này đảm bảo rằng tất cả dữ liệu có trong quá trình tạo bằng chứng và nó cũng yêu cầu các validator phải có toàn bộ dữ liệu được mã hóa để xác minh mọi bằng chứng của mọi danh tính. Vì vậy, không gian cần thiết để xác thực là `number_of_proofs * data_size`

## Tối ưu hóa với PoH

Cải tiến của chúng tôi về cách tiếp cận này là lấy mẫu ngẫu nhiên các phân đoạn được mã hóa nhanh hơn so với thời gian cần mã hóa và ghi lại hàm băm của các mẫu đó vào sổ cái PoH. Do đó, các phân đoạn giữ nguyên thứ tự chính xác cho mọi PoRep và xác minh có thể truyền dữ liệu và xác minh tất cả các bằng chứng trong một đợt duy nhất. Bằng cách này, chúng ta có thể xác minh nhiều bằng chứng đồng thời, mỗi bằng chứng trên lõi CUDA của chính nó. Tổng không gian cần thiết để xác minh là `1_ledger_segment + 2_cbc_blocks * number_of_identities` với số lượng lõi bằng `number_of_identities`. Chúng ta sử dụng kích thước khối chacha CBC 64-byte.

## Mạng

Các validator cho PoRep là các validator tương tự đang xác minh các giao dịch. Nếu người lưu trữ có thể chứng minh rằng một validator đã xác minh một PoRep giả mạo, thì validator đó sẽ không nhận được phần thưởng cho kỷ nguyên lưu trữ đó.

Người lưu trữ là _những khách hàng nhẹ_. Họ tải xuống một phần của sổ cái \(hay còn gọi là Phân đoạn\) và lưu trữ nó, đồng thời cung cấp PoReps về việc lưu trữ sổ cái. Đối với mỗi người lưu trữ PoRep được xác minh sẽ kiếm được phần thưởng là sol từ mining pool.

## Ràng buộc

Chúng tôi có những ràng buộc sau:

- Việc xác minh yêu cầu tạo ra các khối CBC. Điều đó yêu cầu không gian của 2

  khối cho mỗi danh tính và 1 lõi CUDA cho mỗi danh tính cho cùng một tập dữ liệu. Vì vậy, như

  nhiều danh tính cùng một lúc nên được trộn với càng nhiều bằng chứng cho những

  danh tính được xác minh đồng thời cho cùng một tập dữ liệu.

- Các validator sẽ lấy mẫu ngẫu nhiên tập hợp các bằng chứng lưu trữ cho tập hợp

  họ có thể xử lý và chỉ những người tạo ra các bằng chứng đã chọn đó mới được

  được thưởng. Validator có thể chạy điểm chuẩn bất cứ khi nào cấu hình phần cứng của nó

  thay đổi để xác định tỷ lệ nó có thể xác thực bằng chứng lưu trữ.

## Giao thức xác thực và sao chép

### Hằng số

1. SLOTS_PER_SEGMENT: Số slot trong một phân đoạn dữ liệu sổ cái. Các

   đơn vị lưu trữ cho một người lưu trữ.

2. NUM_KEY_ROTATION_SEGMENTS: Số lượng phân đoạn mà sau đó người lưu trữ

   tạo lại các khóa mã hóa của chúng và chọn một tập dữ liệu mới để lưu trữ.

3. NUM_STORAGE_PROOFS: Số lượng bằng chứng lưu trữ bắt buộc để có bằng chứng lưu trữ

   yêu cầu được thưởng thành công.

4. RATIO_OF_FAKE_PROOFS: Tỷ lệ bằng chứng giả so với bằng chứng thực cho thấy một bộ lưu trữ

   yêu cầu bằng chứng khai thác phải có lữu trữ để một phần thưởng có giá trị.

5. NUM_STORAGE_SAMPLES: Số lượng mẫu cần thiết cho khai thác lưu trữ

   bằng chứng.

6. NUM_CHACHA_ROUNDS: Số vòng mã hóa được thực hiện để tạo

   trạng thái được mã hóa.

7. NUM_SLOTS_PER_TURN: Số slot xác định một kỷ nguyên lưu trữ duy nhất hoặc

   một "lượt" của trò chơi PoRep.

### Hành vi của validator

1. Các validator tham gia mạng và bắt đầu tìm kiếm tài khoản người lưu trữ tại mỗi

   kỷ nguyên lưu trữ/ranh giới lần lượt.

2. Mỗi lượt, Các validator ký giá trị PoH tại ranh giới và sử dụng chữ ký đó

   để chọn ngẫu nhiên bằng chứng để xác minh từ mỗi tài khoản lưu trữ được tìm thấy trong ranh giới lượt.

   Giá trị đã ký này cũng được gửi đến tài khoản lưu trữ của validator và sẽ được sử dụng bởi

   người lưu trữ ở giai đoạn sau để xác minh chéo.

3. Mỗi `NUM_SLOTS_PER_TURN` các slot validator quảng cáo giá trị PoH. Đây là giá trị

   cũng được cung cấp cho Bộ lưu trữ thông qua giao diện RPC.

4. Đối với lượt N nhất định, tất cả các xác nhận sẽ bị khóa cho đến lượt N + 3 \(khoảng cách 2 lượt /kỷ nguyên\).

   Tại thời điểm đó, tất cả các xác nhận trong lượt đó đều có sẵn để nhận thưởng.

5. Bất kỳ xác nhận sai nào sẽ bị đánh dấu trong lượt giữa.

### Hành vi của người lưu trữ

1. Vì trình lưu trữ là một khách hàng nhẹ và không tải xuống tất cả

   dữ liệu sổ cái, chúng phải dựa vào các validator và trình lưu trữ khác để có thông tin.

   Bất kỳ validator nào đã cho đều có thể độc hại và cung cấp thông tin không chính xác,

   không có bất kỳ các vector tấn công rõ ràng nào mà điều này có thể thực hiện được ngoài việc có

   người lưu trữ làm thêm công việc lãng phí. Đối với nhiều hoạt động, có một số tùy chọn

   tùy thuộc vào mức độ hoang tưởng của một người lưu trữ:

   - \(a\) người lưu trữ có thể yêu cầu một validator
   - \(b\) người lưu trữ có thể yêu cầu nhiều validator
   - \(c\) người lưu trữ có thể hỏi những người lưu trữ khác
   - \(d\) người lưu trữ có thể đăng ký toàn bộ luồng giao dịch và tạo

     thông tin chính nó \(giả sử slot gần đây là đủ\)

   - \(e\) người lưu trữ có thể đăng ký một luồng giao dịch viết tắt để

     tự tạo thông tin \(giả sử slot gần đây là đủ\)

2. Một trình lưu trữ lấy được hàm băm PoH tương ứng với lượt cuối cùng với slot của nó.
3. Trình lưu trữ ký hàm băm PoH bằng keypair của nó. Chữ ký đó là

   hạt giống được sử dụng để chọn phân đoạn để sao chép và cũng là khóa mã hóa. Các

   trình lưu trữ sửa đổi chữ ký với slot để nhận phân đoạn nào

   nhân rộng.

4. Trình lưu trữ truy xuất lại sổ cái bằng cách yêu cầu các validator ngang hàng và

   người lưu trữ. Xem 6.5.

5. Sau đó, trình lưu trữ mã hóa phân đoạn đó bằng khóa với thuật toán chacha

   ở chế độ CBC với `NUM_CHACHA_ROUNDS` mã hóa.

6. Trình lưu trữ khởi tạo chacha rng với giá trị PoH gần đây đã ký là

   hạt giống.

7. Trình lưu trữ tạo `NUM_STORAGE_SAMPLES` mẫu trong phạm vi

   kích thước mục nhập và lấy mẫu phân đoạn được mã hóa với sha256 cho 32 byte ở mỗi

   giá trị bù đắp. Lấy mẫu trạng thái sẽ nhanh hơn so với việc tạo mã hóa

   bộ phận.

8. Trình lưu trữ gửi một giao dịch bằng chứng PoRep có chứa trạng thái sha của nó

   khi kết thúc hoạt động lấy mẫu, hạt giống của nó và các mẫu mà nó đã sử dụng để

   leader hiện tại và nó được đưa vào sổ cái.

9. Trong một lượt nhất định, người lưu trữ phải gửi nhiều bằng chứng cho cùng một phân đoạn

   và dựa trên `RATIO_OF_FAKE_PROOFS`, một số bằng chứng trong số đó phải là giả mạo.

10. Khi trò chơi PoRep bước vào lượt tiếp theo, người lưu trữ phải gửi

    giao dịch với mặt nạ mà bằng chứng là giả mạo trong lượt trước. Điều này

    giao dịch sẽ xác định phần thưởng cho cả những người lưu trữ và những validator.

11. Cuối cùng đến lượt N, khi trò chơi PoRep bước sang lượt N + 3, bằng chứng của người lưu trữ cho

    lượt N sẽ được tính vào phần thưởng của họ.

### Trò chơi PoRep

Trò chơi Proof of Replication có 4 giai đoạn chính. Đối với mỗi "lượt", các trò chơi PoRep có thể đang diễn ra nhưng mỗi trò chơi ở một giai đoạn khác nhau.

4 giai đoạn của Trò chơi PoRep như sau:

1. Giai đoạn nộp bằng chứng
   - Người lưu trữ: gửi càng nhiều bằng chứng càng tốt trong giai đoạn này
   - Validator: Không
2. Giai đoạn xác minh bằng chứng
   - Người lưu trữ: Không
   - Validator: Chọn người lưu trữ và xác minh bằng chứng của họ từ lượt trước
3. Giai đoạn thử thách bằng chứng
   - Người lưu trữ: Gửi mặt nạ bằng chứng kèm theo lời biện minh \(đối với bằng chứng giả đã được gửi 2 lượt trước\)
   - Validator: Không
4. Giai đoạn thu thập phần thưởng
   - Người lưu trữ: Thu thập phần thưởng cho 3 lượt trước
   - Validator: Thu thập phần thưởng cho 3 lượt trước

Đối với mỗi lượt của trò chơi PoRep, cả Validator và Trình lưu trữ đều đánh giá từng giai đoạn. Các giai đoạn được chạy dưới dạng các giao dịch riêng biệt trên chương trình lưu trữ.

### Tìm người có một khối sổ cái nhất định

1. Các validator giám sát các lượt trong trò chơi PoRep và xem xét ngân hàng gốc

   lần lượt ranh giới cho bất kỳ bằng chứng nào.

2. Các validator duy trì bản đồ các phân đoạn sổ cái và các public key của người lưu trữ tương ứng.

   Bản đồ được cập nhật khi Validator xử lý bằng chứng của người lưu trữ cho một phân đoạn.

   Validator cung cấp giao diện RPC để truy cập vào bản đồ này. Sử dụng API này, khách hàng

   có thể ánh xạ một phân đoạn tới địa chỉ mạng của người lưu trữ \(tương quan với nó qua bảng cluster_info\).

   Sau đó, các khách hàng có thể gửi các yêu cầu sửa chữa đến người lưu trữ để lấy các phân đoạn.

3. Các validator sẽ cần phải làm mất hiệu lực danh sách này sau mỗi N lượt.

## Tấn công sybil

Đối với bất kỳ hạt giống ngẫu nhiên nào, chúng tôi buộc mọi người phải sử dụng một chữ ký có nguồn gốc từ hàm băm PoH ở ranh giới lần lượt. Mọi người sử dụng cùng một số lượng, do đó, cùng một hàm băm PoH được ký bởi mọi người tham gia. Sau đó, mỗi chữ ký được liên kết bằng mật mã với keypair, điều này ngăn leader nghiền ngẫm giá trị kết quả cho nhiều hơn 1 danh tính.

Vì có nhiều nhận dạng khách hàng hơn sau đó là nhận dạng mã hóa, chúng tôi cần chia phần thưởng cho nhiều khách hàng và ngăn chặn các cuộc tấn công Sybil tạo ra nhiều khách hàng để lấy cùng một khối dữ liệu. Để duy trì BFT, chúng tôi muốn tránh một thực thể con người lưu trữ tất cả các bản sao của một đoạn duy nhất của sổ cái.

Giải pháp của chúng tôi là buộc các khách hàng tiếp tục sử dụng cùng một danh tính. Nếu vòng đầu tiên được sử dụng để có được cùng một khối cho nhiều danh tính khách hàng, thì vòng thứ hai cho các danh tính khách hàng giống nhau sẽ buộc phải phân phối lại các chữ ký, do đó nhận dạng và khối PoRep. Do đó, để nhận được phần thưởng, người lưu trữ cần phải lưu trữ miễn phí khối đầu tiên và mạng có thể thưởng cho danh tính khách hàng tồn tại lâu hơn so với các khối mới.

## Các cuộc tấn công validator

- Nếu một validator phê duyệt các bằng chứng giả mạo, người lưu trữ có thể dễ dàng loại bỏ chúng bằng cách

  hiển thị trạng thái ban đầu cho hàm băm.

- Nếu một validator đánh dấu các bằng chứng thực là giả mạo, thì không thể thực hiện tính toán trực tuyến

  để phân biệt ai đúng. Phần thưởng sẽ phải dựa trên kết quả từ

  nhiều validator để bắt các diễn viên và người lưu trữ xấu bị từ chối phần thưởng.

- Validator đánh cắp kết quả bằng chứng khai thác cho chính nó. Các bằng chứng được dẫn xuất

  từ một chữ ký từ một trình lưu trữ, vì validator không biết

  private key được sử dụng để tạo khóa mã hóa, nó không thể là trình tạo của

  bằng chứng.

## Phần thưởng khuyến khích

Bằng chứng giả rất dễ tạo ra nhưng khó xác minh. Vì lý do này, các giao dịch bằng chứng PoRep do trình lưu trữ tạo ra có thể yêu cầu phí cao hơn so với giao dịch bình thường để thể hiện chi phí tính toán mà các validator yêu cầu.

Một số phần trăm bằng chứng giả cũng cần thiết để nhận được phần thưởng từ việc khai thác lưu trữ.

## Ghi chú

- Chúng tôi có thể giảm chi phí xác minh PoRep bằng cách sử dụng PoH và thực sự

  làm cho việc xác minh một số lượng lớn các bằng chứng cho một tập dữ liệu toàn cầu trở nên khả thi.

- Chúng ta có thể loại bỏ việc mài bằng cách buộc mọi người phải ký cùng một hàm băm PoH và

  sử dụng các chữ ký làm hạt giống

- Trò chơi giữa các validator và các người lưu trữ là trên các khối ngẫu nhiên và ngẫu nhiên

  danh tính mã hóa và các mẫu dữ liệu ngẫu nhiên. Mục tiêu của ngẫu nhiên hóa là

  để ngăn các nhóm thông đồng có sự chồng chéo về dữ liệu hoặc xác thực.

- Khách hàng lưu trữ bắt cá cho những validator lười biếng bằng cách gửi bằng chứng giả mạo

  họ có thể chứng minh là giả.

- Để bảo vệ chống lại danh tính khách hàng Sybil cố gắng lưu trữ cùng một khối, chúng tôi

  buộc khách hàng phải lưu trữ nhiều vòng trước khi nhận được phần thưởng.

- Các validator cũng sẽ nhận được phần thưởng khi xác thực các bằng chứng lưu trữ đã gửi

  như động cơ để lưu trữ sổ cái. Họ chỉ có thể xác thực các bằng chứng nếu họ

  đang lưu trữ phần đó của sổ cái.

# Sao chép sổ cái không được triển khai

Hành vi nhân rộng vẫn chưa được thực hiện.

## Kỷ nguyên lưu trữ

Kỷ nguyên lưu trữ phải là số lượng slot dẫn đến khoảng 100GB-1TB sổ cái được tạo để người lưu trữ lưu trữ. Người lưu trữ sẽ bắt đầu lưu trữ sổ cái khi một đợt fork nhất định có khả năng cao không được khôi phục.

## Hành vi của validator

1. Mỗi NUM_KEY_ROTATION_TICKS nó cũng xác nhận các mẫu nhận được từ

   người lưu trữ. Nó ký hiệu hàm băm PoH tại thời điểm đó và sử dụng như sau

   thuật toán với chữ ký làm đầu vào:

   - 5 bit thấp của byte đầu tiên của chữ ký tạo ra một chỉ mục thành

     một byte bắt đầu khác của chữ ký.

   - Sau đó, validator sẽ xem xét tập hợp các bằng chứng lưu trữ trong đó byte của

     vector trạng thái sha của bằng chứng bắt đầu từ byte thấp khớp chính xác

     với \(các\) byte đã chọn của chữ ký.

   - Nếu tập hợp các bằng chứng lớn hơn mức mà validator có thể xử lý, thì

     tăng lên khớp với 2 byte trong chữ ký.

   - Validator tiếp tục tăng số lượng byte phù hợp cho đến khi

     bộ khả thi được tìm thấy.

   - Sau đó, nó tạo ra một mặt nạ của các bằng chứng hợp lệ và bằng chứng giả và gửi nó đến

     leader. Đây là một giao dịch xác nhận bằng chứng lưu trữ.

2. Sau khoảng thời gian khóa NUM_SECONDS_STORAGE_LOCKOUT giây,

   validator sau đó gửi một giao dịch yêu cầu bằng chứng lưu trữ, sau đó gây ra

   phân phối phần thưởng lưu trữ nếu không có thách thức nào được nhìn thấy để làm bằng chứng cho

   bên những validator và những người lưu trữ các bằng chứng.

## Hành vi của người lưu trữ

1. Sau đó, người lưu trữ tạo ra một tập hợp bù đắp khác mà nó gửi đi một giả mạo

   bằng chứng với trạng thái sha không chính xác. Nó có thể được chứng minh là giả bằng cách cung cấp

   hạt giống cho kết quả băm.

   - Một bằng chứng giả phải bao gồm một hàm băm của người lưu trữ của một chữ ký của PoH

     giá trị. Bằng cách đó, khi người lưu trữ tiết lộ bằng chứng giả mạo, nó có thể

     được xác minh trên chuỗi.

2. Người lưu trữ giám sát sổ cái, nếu thấy bằng chứng giả được tích hợp, nó

   tạo một giao dịch thách thức và đệ trình nó cho leader hiện tại. Các

   giao dịch chứng minh validator xác thực không chính xác bằng chứng lưu trữ giả mạo.

   Người lưu trữ được thưởng và số dư staking của validator bị cắt giảm hoặc

   đông cứng.

## Hợp đồng bằng chứng lưu trữ hợp lý

Mỗi trình lưu trữ và validator sẽ có tài khoản lưu trữ riêng. Tài khoản của validator sẽ tách biệt với id gossip của họ tương tự như tài khoản bỏ phiếu của họ. Chúng nên được triển khai dưới dạng hai chương trình, một chương trình xử lý validator làm bộ ký hiệu bàn phím và một chương trình cho trình lưu trữ. Theo cách đó, khi chương trình tham chiếu đến các tài khoản khác, họ có thể kiểm tra id chương trình để đảm bảo đó là tài khoản một validator hoặc tài khoản lưu trữ mà họ đang tham chiếu.

### SubmitMiningProof

```text
SubmitMiningProof {
    slot: u64,
    sha_state: Hash,
    signature: Signature,
};
keys = [archiver_keypair]
```

Các trình lưu trữ tạo những thứ này sau khi khai thác dữ liệu sổ cái được lưu trữ của họ cho một giá trị băm nhất định. Slot là slot cuối cùng của phân đoạn sổ cái mà họ đang lưu trữ, là kết quả của trình lưu trữ bằng cách sử dụng hàm băm để lấy mẫu phân đoạn sổ cái được mã hóa của họ. Chữ ký là chữ ký được tạo khi họ ký giá trị PoH cho kỷ nguyên lưu trữ hiện tại. Danh sách các bằng chứng từ kỷ nguyên lưu trữ hiện tại phải được lưu ở trạng thái tài khoản, sau đó được chuyển sang danh sách các bằng chứng cho kỷ trước đó khi kỷ nguyên đó trôi qua. Trong một kỷ nguyên lưu trữ nhất định, một trình lưu trữ nhất định chỉ nên gửi các bằng chứng cho một phân đoạn.

Chương trình phải có một danh sách các slot là các slot khai thác lưu trữ hợp lệ. Danh sách này nên được duy trì bằng cách theo dõi các slot là các slot đã được root mà một phần đáng kể của mạng đã bỏ phiếu với một giá trị lockout cao, có thể là 32 phiếu cũ. Mỗi SLOTS_PER_SEGMENT số lượng slot sẽ được thêm vào tập hợp này. Chương trình nên kiểm tra xem slot có trong bộ này không. Tập hợp có thể được duy trì bằng cách nhận một AdvertiseStorageRecentBlockHash và kiểm tra với ngân hàng/trạng thái Tower BFT của nó.

Chương trình sẽ thực hiện kiểm tra xác minh chữ ký đối với chữ ký, public key từ người gửi giao dịch và tin nhắn về giá trị PoH của kỷ nguyên lưu trữ trước đó.

### ProofValidation

```text
ProofValidation {
   proof_mask: Vec<ProofStatus>,
}
keys = [validator_keypair, archiver_keypair(s) (unsigned)]
```

Một validator sẽ gửi giao dịch này để chỉ ra rằng một tập hợp các bằng chứng cho một phân đoạn nhất định là hợp lệ/không hợp lệ hoặc bị bỏ qua khi validator không xem xét nó. Các keypair cho các trình lưu trữ mà nó đã xem xét phải được tham chiếu trong các khóa để logic chương trình có thể chuyển đến các tài khoản đó và xem rằng các bằng chứng đã được tạo trong kỷ nguyên trước đó. Việc lấy mẫu của các bằng chứng lưu trữ phải được xác minh để đảm bảo rằng các bằng chứng chính xác được validator bỏ qua theo logic được nêu trong hành vi lấy mẫu của validator.

Các khóa lưu trữ được bao gồm sẽ chỉ ra các mẫu lưu trữ đang được tham chiếu; độ dài của mặt nạ bằng chứng phải được xác minh dựa trên tập hợp các bằng chứng lưu trữ trong \(các\) tài khoản lưu trữ được tham chiếu và phải khớp với số lượng bằng chứng được gửi trong kỷ nguyên lưu trữ trước đó ở trạng thái của tài khoản lưu trữ nói trên.

### ClaimStorageReward

```text
ClaimStorageReward {
}
keys = [validator_keypair or archiver_keypair, validator/archiver_keypairs (unsigned)]
```

Những người lưu trữ và những validator sẽ sử dụng giao dịch này để nhận mã thông báo thanh toán từ một trạng thái chương trình nơi SubmitStorageProof, ProofValidation và ChallengeProofValidations ở trạng thái mà bằng chứng đã được gửi và xác thực và không có ChallengeProofValidations nào tham chiếu đến những bằng chứng đó. Đối với một validator, nó phải tham chiếu đến các keypair của trình lưu trữ mà nó đã xác thực các bằng chứng trong kỷ nguyên liên quan. Và đối với một trình lưu trữ, nó phải tham chiếu đến các keypair của validator mà nó đã xác thực và muốn được thưởng.

### ChallengeProofValidation

```text
ChallengeProofValidation {
    proof_index: u64,
    hash_seed_value: Vec<u8>,
}
keys = [archiver_keypair, validator_keypair]
```

Giao dịch này là để bắt những validator lười biếng không thực hiện công việc xác thực bằng chứng. Một trình lưu trữ sẽ gửi giao dịch này khi thấy một validator đã chấp thuận giao dịch SubmitMiningProof giả mạo. Vì trình lưu trữ là một khách hàng nhẹ không nhìn vào chuỗi đầy đủ, nó sẽ phải yêu cầu một validator hoặc một số validator cho thông tin này có thể thông qua lệnh gọi RPC để lấy tất cả ProofValidations cho một phân đoạn nhất định trong kỷ nguyên lưu trữ trước đó. Chương trình sẽ xem xét trạng thái tài khoản của validator và thấy rằng ProofValidation được gửi trong kỷ nguyên lưu trữ trước đó và hàm băm hash_seed_value và thấy rằng hàm băm khớp với giao dịch SubmitMiningProof và validator đã đánh dấu nó là hợp lệ. Nếu vậy, nó sẽ lưu thử thách vào danh sách các thử thách mà nó có ở trạng thái của nó.

### AdvertiseStorageRecentBlockhash

```text
AdvertiseStorageRecentBlockhash {
    hash: Hash,
    slot: u64,
}
```

Những alidator và những trình lưu trữ sẽ gửi thông báo này để chỉ ra rằng một kỷ nguyên lưu trữ mới đã trôi qua và các bằng chứng lưu trữ là các bằng chứng hiện tại bây giờ sẽ dành cho kỷ nguyên trước đó. Các giao dịch khác nên kiểm tra xem kỷ nguyên mà họ đang tham chiếu có chính xác theo trạng thái chuỗi hiện tại hay không.
