# Return data from BPF programs

## Problem

In the Solidity langauge it is permitted to return any number of values from a function,
for example a variable length string can be returned:

```
function foo1() public returns (string) {
    return "Hello, world!\n";
}
```

Multiple values, arrays and structs are permitted too.

```
struct S {
    int f1;
    bool f2
};

function foo2() public returns (string, int[], S) {
    return (a, b, c);
}
```

All the return values are eth abi encoded to a variable-length byte array.

On ethereum errors can be returned too:

```
function withdraw() public {
    require(msg.sender == owner, "Permission denied");
}

function failure() public {
    revert("I afraid I can't do that dave");
}
```
These errors help the developer debug any issue they are having, and can
also be caught in a Solidity `try` .. `catch` block. Outside of a `try` .. `catch`
block, any of these would cause the transaction or rpc to fail.

## Existing solution

The existing solution that Solang uses, writes the return data to the callee account data.
The caller's account cannot be used, since the callee may not be the same BPF program, so
it will not have permission to write to the callee's account data.

Another solution would be to have a single return data account which is passed
around through CPI. Again this does not work for CPI as the callee may not have
permission to write to it.

The problem with this solution is:

- It does not work for RPC calls
- It is very racey; a client has to submit the Tx and then retrieve the account
  data. This is not atomic so the return data can be overwritten by another transaction.

## Requirements for Solution

It must work for:

- RPC: An RPC should be able to return any number of values without writing to account data
- Transaction: An transaction should be able to return any number of values without needing to write them account data
- CPI: The callee must "set" return value, and the caller must be able to retrieve it.

## Review of other chains

### Ethereum (EVM)

The `RETURN` opcode allows a contract to set a buffer as a returndata. This opcode takes a pointer to memory and a size. The `REVERT` opcode works similarly but signals that the call failed, and all account data changes must be reverted.

For CPI, the caller can retrieve the returned data of the callee using the `RETURNDATASIZE` opcode which returns the length, and the `RETURNDATACOPY` opcode, which takes a memory destination pointer, offset into the returndata, and a length argument.

Ethereum stores the returndata in blocks.

### Parity Substrate

The return data can be set using the `seal_return(u32 flags, u32 pointer, u32 size)` syscall.
- Flags can be 1 for revert, 0 for success (nothing else defined)
- Function does not return

CPI: The `seal_call()` syscall takes pointer to buffer and pointer to buffer size where return data goes
 - There is a 32KB limit for return data.

Parity Substrate does not write the return data to blocks.

## Rejected Solution

The concept of ephemeral accounts has been proposed a solution for this. This would
certainly work for the CPI case, but this would not work RPC or Transaction case.

## Proposed Solution

The callee can set the return data using a new system call `sol_returndata(u8 *buf, u64 length)`.
This function does not return and signals success for execution of the contract. There is a limit
of 1024 bytes for the returndata.

The caller can retrieve the return data of the callee by a new system call:

```
fn sol_invoke_signed_rust_return_data(
    instruction_addr: *const u8,
    account_infos_addr: *const u8,
    account_infos_len: u64,
    signers_seeds_addr: *const u8,
    signers_seeds_len: u64,
    return_data_addr: *mut u8,
    return_data_length: *mut u64,
) -> u64;

uint64_t sol_invoke_signed_c_return_data(
  const SolInstruction *instruction,
  const SolAccountInfo *account_infos,
  int account_infos_len,
  const SolSignerSeeds *signers_seeds,
  int signers_seeds_len
  uint8_t *return_data,
  uint64_t *return_data_length,
);
```
On entry, `return_data_length` should point to the size of the buffer at `return_data`. If the callee
sets more data than this buffer, an error should be returned. On successful exit, `return_data_length`
will contain the actual length returned by the callee.

For a normal RPC or Transaction, the returndata is base64-encoded and stored along side the sol_log
strings.

## Note on returning errors

Solidity on Ethereum allows the contract to return an error in the return data. In this case, all
the account data changes for the account should be reverted. On Solana, any non-zero exit code
for a BPF prorgram means the entire transaction fails. We do not wish to support an error return
by returning success and then returning an error in the return data. This would mean we would have
to support reverting the account data changes; this too expensive both on the VM side and the BPF
contract side.

Errors will be reported via sol_log.
