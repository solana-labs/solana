# Return data from SBF programs

## Problem

In the Solidity language it is permitted to return any number of values from a function,
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
The caller's account cannot be used, since the callee may not be the same SBF program, so
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

The callee can set the return data using a new system call `sol_set_return_data(buf: *const u8, length: u64)`.
There is a limit of 1024 bytes for the returndata. This function can be called multiple times, and
will simply overwrite what was written in the last call.

The return data can be retrieved with `sol_get_return_data(buf: *mut u8, length: u64, program_id: *mut Pubkey) -> u64`.
This function copies the return buffer, and the program_id that set the return data, and
returns the length of the return data, or `0` if no return data is set. In this case, program_id is not set.

When an instruction calls `sol_invoke()`, the return data of the callee is copied into the return data
of the current instruction. This means that any return data is automatically passed up the call stack,
to the callee of the current instruction (or the RPC call).

Note that `sol_invoke()` clears the returns data before invoking the callee, so that any return data from
a previous invoke is not reused if the invoked fails to set a return data. For example:

 - A invokes B
 - Before entry to B, return data is cleared.0
 - B sets some return data and returns
 - A invokes C
 - Before entry to C, return data is cleared.
 - C does not set return data and returns
 - A checks return data and finds it empty

Another scenario to consider:

 - A invokes B
 - B invokes C
 - C sets return data and returns
 - B does not touch return data and returns
 - A gets return data from C
 - A does not touch return data
 - Return data from transaction is what C set.

The compute costs are calculated for getting and setting the return data using
the syscalls.

For a normal RPC or Transaction, the returndata is base64-encoded and stored along side the sol_log
strings in the [stable log](https://github.com/solana-labs/solana/blob/95292841947763bdd47ef116b40fc34d0585bca8/sdk/src/process_instruction.rs#L275-L281).

## Note on returning errors

Solidity on Ethereum allows the contract to return an error in the return data. In this case, all
the account data changes for the account should be reverted. On Solana, any non-zero exit code
for a SBF prorgram means the entire transaction fails. We do not wish to support an error return
by returning success and then returning an error in the return data. This would mean we would have
to support reverting the account data changes; this too expensive both on the VM side and the SBF
contract side.

Errors will be reported via sol_log.
