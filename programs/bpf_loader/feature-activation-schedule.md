# Feature Activation Schedule

The scheduling of feature activations should be based on the following principals:

- Relative priority
- The same order on each network

Those two principals will not always agree, exceptions will occur.

Feature activation so far has been ad-hoc, to help organize and communicate the upcoming feature activations the following table will indicate which features will be activated next and on which networks.

- Features are ordered by priority, highest at the top, priority might change and circumvent activation .
- When a feature is activated on a network, the epoch will be added to the table by the person who activated the feature.
- Once a feature is activated on mainnet the person who activated the feature must remove it from the table.
- Feature owners are responsible for inserting their features into this list following the above principals, exceptions will occur, get consensus from others before re-ordering priority.
- Some features will need to be soaked before being activated on the next network, soaking will delay all lower-priority features, exceptions will occur.
- Features should be activated one at a time per network, exceptions will occur.


To determine which feature will be activated next:

- Features must be activated on Testnet, then Devnet, then mainnet
- For each network, the next feature that has not been enabled will be enabled next


For example, given the following schedule:
| Key | Testnet | Devnet | Description |
|-----|---------|--------|------------|
| 7GUcYgq4tVtaqNCKT3dho9r4665Qp5TxCZ27Qgjx3829 | 331 | 332 | Executables incur CPI data costs |
| DpJREPyuMZ5nDfU6H3WTqSqUFSXAfw8u7xqmWtEwJDcP | 333 |     | quick bail on panic |
| 7g9EUwj4j7CS21Yx1wvgWLjSZeh5aPq8x9kpoPwXM8n8 | 334 |     | limit secp256k1 recovery id |
| nWBqjr3gpETbiaVj3CBJ3HFC5TMdnJDGt21hnvSTvVZ  |     |     | check physical overlapping regions |

`nWBqjr3gpETbiaVj3CBJ3HFC5TMdnJDGt21hnvSTvVZ will be activated on devnet next`
`DpJREPyuMZ5nDfU6H3WTqSqUFSXAfw8u7xqmWtEwJDcP will be activated on testnet next`
`7GUcYgq4tVtaqNCKT3dho9r4665Qp5TxCZ27Qgjx3829 will be activated on mainnet next`

## Current Schedule

| Key | Testnet | Devnet | Description |
|-----|---------|--------|-------------|
| 7GUcYgq4tVtaqNCKT3dho9r4665Qp5TxCZ27Qgjx3829 | 331 | 332 | Executables incur CPI data costs |
| DpJREPyuMZ5nDfU6H3WTqSqUFSXAfw8u7xqmWtEwJDcP | 333 |     | quick bail on panic |
| 98std1NSHqXi9WYvFShfVepRdCoq1qvsp8fsR2XZtG8g | 332 | 332 | add compute budget ix for setting a compute unit price |
| J2QdYx8crLbTVK8nur1jeLsmc3krDbfjoxoea2V1Uy5Q | 326 | 328 | Default max tx-wide compute units calculated per instruction |
| 8aXvSuopd1PUj7UhehfXJRg6619RHp8ZvwTyyJHdUYsj | 277 | 297 | fail libsec256k1_verify if count appears wrong |
| 7g9EUwj4j7CS21Yx1wvgWLjSZeh5aPq8x9kpoPwXM8n8 | 334 |     | limit secp256k1 recovery id |
| 4ApgRX3ud6p7LNMJmsuaAcZY5HWctGPr5obAsjB3A54d | 277 | 297 | prevent calling precompiles as programs |
| 3aJdcZqxoLpSBxgeYGjPwaYS1zzcByxUDqJkbzWAH1Zb | 334 |     | move the CPI stack overflow check to the end of push |
| nWBqjr3gpETbiaVj3CBJ3HFC5TMdnJDGt21hnvSTvVZ  |     |     | check physical overlapping regions |
| HyrbKftCdJ5CrUfEti6x26Cj7rZLNe32weugk7tLcWb8 |     |     | syscalls use saturated math |
| 21AWDosvp3pBamFW91KB35pNoaoZVTM7ess8nr2nt53B | 212 | 163 | merge NonceError into SystemError |
| HTW2pSyErTj4BV6KBM9NZ9VBUJVxt7sacNWcf76wtzb3 | 277 |     | blake3 syscall |
| 7rcw5UtqgDTBBv2EcynNfYckgdAaH1MAsCjKgXMkN7Ri |     |     | enable curve25519 syscalls |
| GTUMCZ8LTNxVfxdrw7ZsDFTxXb7TutYkzJnFwinpE6dg |     |     | disable the deprecated BPF loader |
| 2jXx2yDmGysmBKfKYNgLj2DQyAQv6mMk2BPh4eSbyB4H | 277 |     | deprecate fee calculator |
