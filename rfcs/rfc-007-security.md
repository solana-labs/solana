# Security

The goal of this RFC is to define a set of design constraints that are focused on limiting software attack vectors, and not economic attack vectors. 

## Version

Version 0.1 
 
## Attacks Goals

Committing an invalid state, such as tokens created or destroyed. Denial of service for the network to accept transactions, get to consensus, globally or for single node or program.

## Attacks
1. Every validator signs an invalid state.  This kind of attack commits economic finality into an incorrect state.  Examples:
    1. An exploit in the VM causes a transaction to create tokens.  Every validator signs this state and continues without noticing.
    2. A rowhammer attack leaks the same values to all validators which sign the same state
    3. A network exploit causes an arbitrary execution of code.  This is particularly problematic because an attack could the code they want to execute as a contract, then use a side-channel as an exploit to cause all the validators to execute the same code path.
    4. Network based exploit allows for deterministic state corruption.
    5. Signing verificaiton failure in validators.
2. Every validator signs a different state.  This attack creates a denial of service, since all the validators diverge in state
    1. A row hammer attack causes memory to leak, if each VM memory is randomized and the result is unpredictable
    2. An exploit in the VM for programs allows for a program to read a raw system pointer as a value.  Since each raw pointed is different between validators the result is a dividing state.
    3. Network based exploit allows for random state corruption.
3. Node signs a slash-able vote
    1. Exploit causes a nod to sign an invalid fork.
4. Leader crash that generates a corrupted transactional ledger but not PoH.  This would cause a denial of service if every leader in the secheudle was impacted by this.
    1. Exploit in the VM allows for transactions that fail verification to be encoded into the ledger
5. Leader produces transactional data that crashes validators
    1. Exploit in the VM causes crashes in the validators.


## VM requirements

Memory accesses are guaranteed to succeed or abort. Pointer values or any extra-VM state cannot be leaked into the VM state.  Jump's or static analysis can track and guarantee execution time.

## Key Sandboxing

To prevent Attack 3, each functional state of the node should be controlled by a different key.

1. Stake
    1. Stake Owner 
    2. Stake Operator 
2. Leader
    1. Enclave that signs Entries
    2. Node that processes transactions
3. Validator
    1. Enclave that signs a state vote
    2. Validator that processes transactions

### Stake owner

Actual key that controls stake ownership.
1. Assign a Stake Operator
2. Withdraw tokens

### Stake Operator
1. Assign Leaders and a stake portion per Leader for scheduling
2. Assign Validators and stake portion for validation


### Validator/Leader Node
Actual node key that is broadcast through gossip.  This represents the machine instance.  This key is used for signing network messages, nodes can use QoS based on Stake Owner weight.

### Validator/Leader Enclave
Enclave, signer of the state or entries.  This operation could be slashed.  The enclaves for each node type should be doing the smallest amount of work to validate PoH and ensure that a slashing condition has not been reached.
