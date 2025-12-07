# Monero-Oxide Migration Plan

## Overview

This document outlines the migration strategy from `monero-rs` to `monero-oxide` while maintaining backwards compatibility and not breaking the project.

## Current State

### Completed ✅
- Added `monero-oxide`, `monero-primitives`, `monero-address` to workspace
- Created compatibility test suite (13 tests passing)
- Added `network_oxide` serde module
- Migrated `monero-rpc-pool` to monero-oxide

### Verified Compatibility ✅
| Type | Serialization | Status |
|------|---------------|--------|
| Network | JSON/CBOR identical | ✅ Verified |
| Address | String format identical | ✅ Verified |
| PrivateKey | 32-byte representation identical | ✅ Verified |
| Amount | u64 piconeros | ✅ Compatible |

## Architecture Analysis

```
                    ┌─────────────────────────────────────────┐
                    │              User Interface              │
                    └─────────────────────────────────────────┘
                                        │
                    ┌─────────────────────────────────────────┐
                    │                  swap                    │
                    │         (main application logic)         │
                    └─────────────────────────────────────────┘
                         │              │              │
        ┌────────────────┼──────────────┼──────────────┼────────────────┐
        ▼                ▼              ▼              ▼                ▼
┌──────────────┐  ┌────────────┐ ┌────────────┐ ┌────────────┐  ┌────────────┐
│ swap-machine │  │  swap-db   │ │  swap-p2p  │ │  swap-env  │  │ swap-serde │
│ (state mach) │  │ (database) │ │ (protocol) │ │  (config)  │  │  (serde)   │
└──────────────┘  └────────────┘ └────────────┘ └────────────┘  └────────────┘
        │                │              │              │                │
        └────────────────┴──────────────┴──────────────┴────────────────┘
                                        │
                    ┌─────────────────────────────────────────┐
                    │              swap-core                   │
                    │   (core types: Amount, Address, Keys)    │
                    └─────────────────────────────────────────┘
                                        │
                    ┌─────────────────────────────────────────┐
                    │              monero-sys                  │
                    │      (C++ bindings to Monero wallet)     │
                    │   Returns: Address, Amount, PrivateKey   │
                    └─────────────────────────────────────────┘
```

## Migration Strategy: Bottom-Up with Compatibility Layer

### Phase 1: Expand Serde Support (Current Phase)
**Goal:** Add monero-oxide serde helpers to swap-serde

**Files to modify:**
- `swap-serde/src/monero.rs` - Add oxide helpers

**New modules to add:**
```rust
// Already done:
pub mod network_oxide { ... }  // ✅ Done

// Need to add:
pub mod private_key_oxide { ... }  // For curve25519_dalek::Scalar
pub mod address_oxide { ... }      // For monero_address::MoneroAddress
```

**Risk:** Low - additive changes only

---

### Phase 2: Create Type Bridge in swap-core
**Goal:** Create a compatibility layer that can work with both libraries

**Approach:** Instead of changing existing types, create new "oxide" versions alongside:

```rust
// swap-core/src/monero/oxide.rs (NEW FILE)

/// Re-exports from monero-oxide for use in the codebase
pub use monero_address::Network;
pub use monero_address::MoneroAddress as Address;

/// Scalar type for private keys (monero-oxide uses curve25519-dalek v4)
pub type Scalar = curve25519_dalek::scalar::Scalar;
pub type EdwardsPoint = curve25519_dalek::edwards::EdwardsPoint;

/// Conversion from monero-rs types to monero-oxide types
pub mod convert {
    use super::*;
    
    pub fn network_to_oxide(n: monero::Network) -> Network {
        match n {
            monero::Network::Mainnet => Network::Mainnet,
            monero::Network::Stagenet => Network::Stagenet,
            monero::Network::Testnet => Network::Testnet,
        }
    }
    
    pub fn network_from_oxide(n: Network) -> monero::Network {
        match n {
            Network::Mainnet => monero::Network::Mainnet,
            Network::Stagenet => monero::Network::Stagenet,
            Network::Testnet => monero::Network::Testnet,
        }
    }
    
    // Address conversion via string (verified compatible)
    pub fn address_to_oxide(addr: &monero::Address) -> Result<Address, Error> {
        Address::from_str(network_to_oxide(addr.network), &addr.to_string())
    }
    
    // etc.
}
```

**Risk:** Low - additive, doesn't change existing code

---

### Phase 3: Update monero-sys Return Types
**Goal:** Have monero-sys use monero-oxide types internally

**Key insight:** monero-sys gets data from C++ as:
- Addresses: strings (parsed via `Address::from_str`)
- Amounts: u64 (piconeros)
- Private keys: 32-byte arrays

**Changes:**
1. Parse addresses to `monero_address::MoneroAddress`
2. Use `u64` for amounts (create wrapper if needed)
3. Use `curve25519_dalek::Scalar` for private keys

**Files:**
- `monero-sys/src/lib.rs`
- `monero-sys/Cargo.toml`

**Risk:** Medium - core changes, but well-tested interfaces

---

### Phase 4: Migrate swap-core Types
**Goal:** Update protocol types to use monero-oxide

**Order of migration (lowest to highest risk):**

1. **Amount** - Already using custom type, just update `From` impls
2. **Network** - Simple enum, already have oxide serde
3. **Address** - Used in many places, careful migration
4. **PrivateKey/PublicKey** - Most critical, serialization must match

**Serialization compatibility:**
- Run existing tests before and after
- Add golden tests for serialized data

**Risk:** Medium-High - core types, but we have compatibility tests

---

### Phase 5: Migrate Dependent Crates
**Order:**
1. `swap-serde` (update remaining helpers)
2. `swap-env` (simple config)
3. `swap-db` (database types)
4. `swap-machine` (state machine)
5. `swap-p2p` (protocol messages)
6. `swap` (main crate)
7. `monero-harness` (tests)
8. `monero-rpc` (if needed, may deprecate)

**Risk:** Low per crate, but many changes

---

### Phase 6: Cleanup
**Goal:** Remove monero-rs dependency

1. Remove `monero` from workspace dependencies
2. Remove `[patch.crates-io]` for monero
3. Remove any remaining monero-rs imports
4. Remove compatibility/conversion code

**Risk:** Low - just cleanup

---

## Rollback Strategy

At each phase, we can rollback by:
1. Reverting git commits
2. Both libraries remain available during migration
3. Conversion functions allow mixing types if needed

## Testing Strategy

### Before Each Phase:
```bash
cargo test --workspace
```

### Serialization Golden Tests:
Create files with known serialized data:
```rust
#[test]
fn golden_private_key_cbor() {
    let expected = include_bytes!("golden/private_key.cbor");
    // ... verify new code produces identical output
}
```

### Integration Tests:
- Run full swap tests between phases
- Verify database compatibility
- Test network protocol compatibility

## Timeline Estimate

| Phase | Effort | Risk |
|-------|--------|------|
| Phase 1: Serde Support | 1-2 hours | Low |
| Phase 2: Type Bridge | 2-3 hours | Low |
| Phase 3: monero-sys | 4-6 hours | Medium |
| Phase 4: swap-core | 4-6 hours | Medium-High |
| Phase 5: Dependent Crates | 4-8 hours | Low |
| Phase 6: Cleanup | 1-2 hours | Low |

**Total: 16-27 hours**

## Files Changed Per Phase

### Phase 1
- `swap-serde/src/monero.rs` (modify)

### Phase 2
- `swap-core/src/monero/oxide.rs` (new)
- `swap-core/src/monero/mod.rs` (modify)
- `swap-core/Cargo.toml` (modify)

### Phase 3
- `monero-sys/src/lib.rs` (modify)
- `monero-sys/Cargo.toml` (modify)

### Phase 4
- `swap-core/src/monero/primitives.rs` (modify)
- `swap-core/src/monero/ext.rs` (modify)

### Phase 5
- Multiple files across 7+ crates

### Phase 6
- `Cargo.toml` (workspace)
- Remove conversion code

## Next Steps

1. Review and approve this plan
2. Start Phase 1: Expand serde support
3. Create golden test files for serialization
4. Proceed phase by phase with tests between each

