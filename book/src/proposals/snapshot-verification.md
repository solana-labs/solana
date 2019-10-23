# Snapshot Verification

## Problem

Snapshot verification of the account states is implemented, but the bank hash of the snapshot which is used to verify is falsifiable.

## Solution

Use the simple payment verification (SPV) solution to verify the vote transactions which are on-chain voting for the bank hash value.
