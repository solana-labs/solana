
[ejson](https://github.com/Shopify/ejson) and
[ejson2env](https://github.com/Shopify/ejson2env) are used to manage access
tokens and other secrets required for CI.

#### Cài đặt
```bash
$ sudo gem install ejson ejson2env
```

sau đó lấy "keypair" và copy vào `/opt/ejson/keys/`.

#### Sử dụng
Chạy lệnh sau để giải mã "secrets" vào môi trường:
```bash
eval $(ejson2env secrets.ejson)
```

#### Quản lý secrets.ejson
Để giải mã `secrets.ejson` cho mục đích chỉnh sửa, chạy lệnh:
```bash
$ ejson decrypt secrets.ejson -o secrets_unencrypted.ejson
```

Chỉnh sửa, sau đó chạy lệnh sau để mật mã file **TRƯỚC KHI ĐƯA MÃ NGUỒN CỦA BẠN LÊN
**:
```bash
$ ejson encrypt secrets_unencrypted.ejson
$ mv secrets_unencrypted.ejson secrets.ejson
```

