---
title: Gửi và nhận mã thông báo
---

Trang này hướng dẫn cách nhận và gửi mã thông báo SOL bằng cách sử dụng các công cụ dòng lệnh với các ví dòng lệnh như [ví giấy](../wallet-guide/paper-wallet.md), [ví hệ thống tệp](../wallet-guide/file-system-wallet.md), hoặc [ví phần cứng](../wallet-guide/hardware-wallets.md). Trước khi bắt đầu, hãy đảm bảo rằng bạn đã tạo một ví và có quyền truy cập vào địa chỉ của nó (pubkey) và keypair xác nhận. Kiểm tra [ các quy ước của chúng tôi để nhập keypair cho các loại ví khác nhau ](../cli/conventions.md#keypair-conventions).

## Kiểm tra ví của bạn

Trước khi chia sẻ public key của bạn với những người khác, việc đầu tiên bạn cần phải làm là phải đảm bảo ví hợp lệ và bạn thực sự nắm giữ private key của ví.

Trong ví dụ này, chúng tôi sẽ tạo ví thứ hai từ ví đầu tiên của bạn và sau đó sẽ chuyển một số mã thông báo vào đó. Điều này xác nhận rằng là bạn có thể gửi và nhận mã thông báo trên ví đó.

Ví dụ thử nghiệm này sử dụng Developer Testnet của chúng tôi, được gọi tắt là devnet. Mã thông báo được phát hành trên mạng devnet **không** có giá trị, vì vậy đừng lo lắng nếu bạn mất nó.

#### Nhận airdrop cho mình một số mã thông báo để bắt đầu

Đầu tiên, hãy nhận _airdrop_ cho mình một số mã thông báo trên devnet.

```bash
solana airdrop 1 <RECIPIENT_ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

`<RECIPIENT_ACCOUNT_ADDRESS>` nơi bạn thay thế văn bản bằng public key/wallet address được mã hóa base58 của mình.

#### Kiểm tra số dư của bạn

Xác nhận đã nhận airdrop thành công bằng cách kiểm tra số dư tài khoản. Nó sẽ hiện ra `1 SOL`:

```bash
solana balance <ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

#### Tạo địa chỉ ví thứ hai

Chúng tôi cần một địa chỉ mới để bạn có thể nhận mã thông báo của mình. Tạo keypair thứ hai và ghi lại pubkey của nó:

```bash
solana-keygen new --no-passphrase --no-outfile
```

Đầu ra sẽ hiển thị địa chỉ của bạn sau văn bản `pubkey:`. Sao chép lại địa chỉ của bạn. Chúng ta sẽ sử dụng nó trong các bước tiếp theo.

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

Bạn cũng có thể tạo ví thứ hai (hoặc nhiều hơn) của bất kỳ loại ví nào: [ví giấy](../wallet-guide/paper-wallet#creating-multiple-paper-wallet-addresses), [ví hệ thống tệp](../wallet-guide/file-system-wallet.md#creating-multiple-file-system-wallet-addresses), or [ví phần cứng](../wallet-guide/hardware-wallets.md#multiple-addresses-on-a-single-hardware-wallet).

#### Chuyển mã thông báo từ ví đầu tiên của bạn sang địa chỉ thứ hai

Tiếp theo, hãy chứng minh rằng bạn đang sở hữu mã thông đã được airdrop bằng cách chuyển chúng. Cụm Solana sẽ chỉ chấp nhận chuyển khoản khi bạn xác nhận giao dịch bằng keypair tương ứng với public key của người gửi trong giao dịch.

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> 0.5 --allow-unfunded-recipient --url https://devnet.solana.com --fee-payer <KEYPAIR>
```

`<KEYPAIR>` nơi để bạn thay thế bằng đường dẫn đến keypair trong ví đầu tiên của mình, và hãy thay thế `<RECIPIENT_ACCOUNT_ADDRESS>` bằng địa chỉ ví thứ hai của bạn.

Xác nhận số dư đã cập nhật với `solana balance`:

```bash
solana balance <ACCOUNT_ADDRESS> --url http://devnet.solana.com
```

trong đó `<ACCOUNT_ADDRESS>` là public key từ keypair của bạn hoặc public key của người nhận.

#### Ví dụ đầy đủ về chuyển thử nghiệm

```bash
$ solana-keygen new --outfile my_solana_wallet.json   # Creating my first wallet, a file system wallet
Generating a new keypair
For added security, enter a passphrase (empty for no passphrase):
Wrote new keypair to my_solana_wallet.json
==========================================================================
pubkey: DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK                          # Here is the address of the first wallet
==========================================================================
Save this seed phrase to recover your new keypair:
width enhance concert vacant ketchup eternal spy craft spy guard tag punch    # If this was a real wallet, never share these words on the internet like this!
==========================================================================

$ solana airdrop 1 DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com  # Airdropping 1 SOL to my wallet's address/pubkey
Requesting airdrop of 1 SOL from 35.233.193.70:9900
1 SOL

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com # Check the address's balance
1 SOL

$ solana-keygen new --no-outfile  # Creating a second wallet, a paper wallet
Generating a new keypair
For added security, enter a passphrase (empty for no passphrase):
====================================================================
pubkey: 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv                   # Here is the address of the second, paper, wallet.
====================================================================
Save this seed phrase to recover your new keypair:
clump panic cousin hurt coast charge engage fall eager urge win love   # If this was a real wallet, never share these words on the internet like this!
====================================================================

$ solana transfer --from my_solana_wallet.json 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv 0.5 --allow-unfunded-recipient --url https://devnet.solana.com --fee-payer my_solana_wallet.json  # Transferring tokens to the public address of the paper wallet
3gmXvykAd1nCQQ7MjosaHLf69Xyaqyq1qw2eu1mgPyYXd5G4v1rihhg1CiRw35b9fHzcftGKKEu4mbUeXY2pEX2z  # This is the transaction signature

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com
0.499995 SOL  # The sending account has slightly less than 0.5 SOL remaining due to the 0.000005 SOL transaction fee payment

$ solana balance 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv --url https://devnet.solana.com
0.5 SOL  # The second wallet has now received the 0.5 SOL transfer from the first wallet

```

## Nhận Mã thông báo

Để nhận mã thông báo, bạn cần một địa chỉ ví để người khác có thể gửi mã thông báo đến bạn. Trong Solana, địa chỉ ví là public key của keypair. Có nhiều cách để tạo keypair. Phương pháp bạn chọn sẽ phụ thuộc vào cách bạn chọn để lưu trữ keypair. Các Keypair được lưu trữ trong ví. Trước khi nhận mã thông báo, bạn cần phải [tạo ví](../wallet-guide/cli.md). Sau khi hoàn tất, bạn sẽ có public key cho keypair mà bạn đã tạo. Public key sẽ là một chuỗi dài gồm các ký tự base58. Độ dài của nó thay đổi từ 32 đến 44 ký tự.

## Gửi Mã thông báo

Nếu bạn đang giữ SOL và muốn gửi mã thông báo cho một ai đó, bạn cần một đường dẫn đến keypair của mình, public key được mã hóa base58 của họ và bạn cần có token để chuyển. Khi bạn đáp ứng được đủ các yêu cầu đó, bạn có thể chuyển mã thông báo bằng lệnh `solana transfer`

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> <AMOUNT> --fee-payer <KEYPAIR>
```

Xác nhận số dư đã cập nhật với lệnh `solana balance`:

```bash
solana balance <ACCOUNT_ADDRESS>
```
