# EIP-8141 adapter changes

## Fallible receipt construction and commit

`ReceiptBuilder::build_receipt` now returns
`Result<Self::Receipt, BlockExecutionError>`.
Existing builders should wrap successful receipts in `Ok(receipt)` and propagate
construction errors. A frame/result mismatch returns
`InternalBlockExecutionError::ReceiptTypeMismatch`, not a fabricated receipt or
a panic. This is an adapter error, not a peer/block validation error.

`BlockExecutor::commit_transaction` now returns
`Result<GasOutput, BlockExecutionError>`. Callers must handle the error (usually
with `?`); custom implementations must construct the receipt before updating
gas counters, receipt lists, or database state. The Ethereum executor does this
and leaves those values unchanged on receipt errors.

## Configured gas schedules

The conversion traits have additive `*_with_gas_params` methods. Their defaults
preserve existing custom implementations. Converters that derive transaction
gas limits must override these methods when their derivation depends on the gas
schedule. Wrappers must forward the parameter to their inner converter.

`Evm::transact`, Ethereum block admission, and RPC request conversion now use
the configured schedule. The built-in recovered, encoded, reference and Either
wrappers forward it. Context-free conversion remains available and uses the
default Amsterdam frame gas schedule; use the parameterized method when manually
constructing a frame environment for a custom schedule.

Explicit `TxEnv` inputs (including prebuilt block-execution tuples) are left
unchanged. Incorrect gas limits are rejected by Revm, not silently repaired.

## Owned frame conversion

`tx_env_from_eip8141(tx, gas_params)` consumes a `TxEip8141`, computes its
signature hash before moving its fields, and transfers its frame, signature and
blob-hash vectors into `TxEnv` without cloning them. RPC conversion uses this
path. Borrowed consensus conversion still clones the data it must retain.
