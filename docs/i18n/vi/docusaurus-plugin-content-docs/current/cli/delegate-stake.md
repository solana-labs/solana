---
title: Ủy quyền Stake
---

Sau khi bạn đã [nhận được SOL](transfer-tokens.md), bạn có thể cân nhắc đưa nó vào sử dụng bằng cách ủy quyền _stake_ cho validator. Stake có nghĩa là các mã thông báo có trong _stake account_. Solana coi trọng số phiếu bầu của validator bằng số stake được ủy quyền cho họ, điều này mang lại cho những validator đó nhiều ảnh hưởng hơn trong việc xác định khối giao dịch hợp lệ tiếp theo trong blockchain. Solana sau đó tạo SOL mới theo định kỳ để thưởng cho những người tham gia stake và các validator. Bạn sẽ kiếm được nhiều phần thưởng hơn khi bạn ủy thác nhiều hơn.

## Tạo tài khoản Stake

Để ủy quyền stake, bạn sẽ cần chuyển một số mã thông báo của bạn vào tài khoản stake. Để tạo một tài khoản, bạn sẽ cần một keypair. Public key của nó sẽ được sử dụng làm [địa chỉ của tài khoản stake](../staking/stake-accounts.md#account-address). Không cần mật khẩu hoặc mã hóa ở đây; keypair này sẽ bị hủy ngay sau khi tạo tài khoản stake.

```bash
solana-keygen new --no-passphrase -o stake-account.json
```

Đầu ra sẽ chứa public key sau văn bản `pubkey:`.

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

Sao chép public key và lưu giữ nó giữ an toàn. Bạn sẽ cần nó để thực hiện một số hành động trên tài khoản stake mà bạn sẽ tạo tiếp theo.

Bây giờ, hãy tạo một tài khoản stake:

```bash
solana create-stake-account --from <KEYPAIR> stake-account.json <AMOUNT> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --fee-payer <KEYPAIR>
```

`<AMOUNT>` mã thông báo được chuyển từ tài khoản tại "from" `<KEYPAIR>` sang một tài khoản stake mới tại public key của stake-account.json.

Bây giờ có thể hủy bỏ tệp tin stake-account.json. Để ủy quyền các hành động tiếp theo, bạn phải sử dụng keypair -`--stake-authority` or `--withdraw-authority` keypair, chứ không phải là stake-account.json.

Xem tài khoản stake mới bằng lệnh `solana stake-account`:

```bash
solana stake-account <STAKE_ACCOUNT_ADDRESS>
```

Đầu ra sẽ giống như sau:

```text
Total Stake: 5000 SOL
Stake account is undelegated
Stake Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
```

### Tổ chức phát hành stake và rút tiền

[Stake và rút tiền](../staking/stake-accounts.md#understanding-account-authorities) có thể được thiết lập khi tạo tài khoản thông qua các tùy chọn `--stake-authority` and `--withdraw-authority`, hoặc sau đó bằng lệnh `solana stake-authorize`. Ví dụ, để thiết lập một stake authority mới, hãy chạy:

```bash
solana stake-authorize <STAKE_ACCOUNT_ADDRESS> \
    --stake-authority <KEYPAIR> --new-stake-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

Điều này sẽ sử dụng stake authority hiện có `<KEYPAIR>` để cấp phép cho stake authority mới <`<PUBKEY>` trên tài khoản stake `<STAKE_ACCOUNT_ADDRESS>`.

### Nâng cao: Lấy địa chỉ tài khoản Stake

Khi bạn ủy quyền stake, bạn ủy quyền tất cả các mã thông báo trong tài khoản stake cho một validator duy nhất. Để ủy quyền cho nhiều validator hơn, bạn cần phải có nhiều tài khoản stake. Việc tạo keypair mới cho mỗi tài khoản và quản lý các địa chỉ đó có thể rất phức tạp. May mắn thay, bạn có thể lấy địa chỉ stake bằng cách sử dụng tùy chọn `--seed`:

```bash
solana create-stake-account --from <KEYPAIR> <STAKE_ACCOUNT_KEYPAIR> --seed <STRING> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> --fee-payer <KEYPAIR>
```

`<STRING>` là một chuỗi ngẫu nhiên lên đến 32 byte, nhưng thường sẽ là một số tương ứng với tài khoản dẫn xuất này. Tài khoản đầu tiên có thể là "0", sau đó là "1", v. v. Public key của `<STAKE_ACCOUNT_KEYPAIR>` hoạt động như địa chỉ cơ sở. Lệnh lấy một địa chỉ mới từ địa chỉ cơ sở và chuỗi hạt giống. Để xem lệnh sẽ lấy địa chỉ stake nào, hãy sử dụng `solana create-address-with-seed`:

```bash
solana create-address-with-seed --from <PUBKEY> <SEED_STRING> STAKE
```

`<PUBKEY>` là public key của `<STAKE_ACCOUNT_KEYPAIR>` được chuyển tới `solana create-stake-account`.

Lệnh sẽ xuất ra một địa chỉ dẫn xuất, có thể được sử dụng cho `<STAKE_ACCOUNT_ADDRESS>` trong các hoạt động staking.

## Ủy quyền Stake

Để ủy thác stake của bạn cho validator, bạn cần có địa chỉ tài khoản bỏ phiếu của người đó. Tìm nó bằng cách truy vấn cụm để biết danh sách tất cả các validator và tài khoản của họ bằng lệnh `solana validators`:

```bash
các validator solana
```

Cột đầu tiên của mỗi hàng chứa danh tính của validator và cột thứ hai là địa chỉ tài khoản bỏ phiếu. Chọn một validator và sử dụng địa chỉ tài khoản của họ trong `solana delegate-stake`:

```bash
solana delegate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <VOTE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

Stake authority `<KEYPAIR>` cho phép hoạt động trên tài khoản có địa chỉ `<STAKE_ACCOUNT_ADDRESS>`. Stake được ủy quyền cho tài khoản bỏ phiếu có địa chỉ `<VOTE_ACCOUNT_ADDRESS>`.

Sau khi ủy quyền stake, hãy sử dụng `solana stake-account` để theo dõi những thay đổi đối với tài khoản stake:

```bash
solana stake-account <STAKE_ACCOUNT_ADDRESS>
```

Bạn sẽ thấy "Delegated Stake và "Delegated Vote Account Address" trong đầu ra. Đầu ra sẽ giống như sau:

```text
Total Stake: 5000 SOL
Credits Observed: 147462
Delegated Stake: 4999.99771712 SOL
Delegated Vote Account Address: CcaHc2L43ZWjwCHART3oZoJvHLAe9hzT2DJNUpBzoTN1
Stake activates starting from epoch: 42
Stake Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
```

## Hủy kích hoạt Stake

Sau khi ủy quyền, bạn có thể hủy stake bằng lệnh `solana deactivate-stake`:

```bash
solana deactivate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

Stake authority `<KEYPAIR>` cho phép hoạt động trên tài khoản có địa chỉ `<STAKE_ACCOUNT_ADDRESS>`.

Lưu ý rằng stake cần vài epoch để "cool down". Nỗ lực ủy thác stake trong giai đoạn cool down sẽ không thành công.

## Rút Stake

Chuyển mã thông báo ra khỏi tài khoản stake bằng lệnh `solana withdraw-stake`:

```bash
solana withdraw-stake --withdraw-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <RECIPIENT_ADDRESS> <AMOUNT> \
    --fee-payer <KEYPAIR>
```

` <STAKE_ACCOUNT_ADDRESS>`` là tài khoản stake hiện có, stake authority <code><KEYPAIR> ` là withdraw authority và `<AMOUNT>` là số lượng mã thông báo cần chuyển `<RECIPIENT_ADDRESS>`.

## Chia Stake

Bạn có thể muốn ủy thác tiền stake cho những validator bổ sung trong khi tiền stake hiện có của bạn không đủ điều kiện để rút. Nó có thể không đủ điều kiện vì nó hiện đang được stake, cool down hoặc bị lock. Để chuyển mã thông báo từ tài khoản stake hiện có sang một tài khoản mới, hãy sử dụng lệnh `solana split-stake`:

```bash
solana split-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <NEW_STAKE_ACCOUNT_KEYPAIR> <AMOUNT> \
    --fee-payer <KEYPAIR>
```

` <STAKE_ACCOUNT_ADDRESS>`` là tài khoản stake hiện có, stake authority <code><KEYPAIR> ` là stake authority,`<NEW_STAKE_ACCOUNT_KEYPAIR>` là keypair cho tài khoản mới và `<AMOUNT>` là số lượng mã thông báo để chuyển sang tài khoản mới.

Để chia tài khoản stake thành địa chỉ tài khoản nguồn, hãy sử dụng `--seed`. Xem [Địa chỉ tài khoản Derive Stake](#advanced-derive-stake-account-addresses) để biết thêm chi tiết.
