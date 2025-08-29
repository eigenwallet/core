# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build/Test Commands

- Build: `cargo build`
- Test all: `cargo test`

## Development Notes

- In src/bridge.rs we mirror some functions from monero/src/wallet/api/wallet2_api.h. CXX automatically generates the bindings from the header file.
- When you want to add a new function to the bridge, you need to copy its definition from the monero/src/wallet/api/wallet2_api.h header file into the bridge.rs file, and then add it some wrapping logic in src/lib.rs.

## Code Conventions

- Rust 2021 edition
- Use `unsafe` only for FFI interactions with Monero C++ code
- The cmake build target we need is wallet_api. We need to link libwallet.a and libwallet_api.a.
- When using `.expect()`, the message should be a short description of the assumed invariant in the format of `.expect("the invariant to be upheld")`.

## Important Development Guidelines

- Always verify method signatures in the Monero C++ headers before adding them to the Rust bridge
- Check wallet2_api.h for the correct function names, parameters, and return types
- When implementing new wrapper functions:
  1. First locate the function in the original C++ header file (wallet2_api.h)
  2. Copy the exact method signature to bridge.rs
  3. Implement the Rust wrapper in lib.rs
  4. Run the build to ensure everything compiles correctly
- Remember that some C++ methods may be overloaded or have different names than expected

## Bridge Architecture

The bridge between Rust and the Monero C++ code works as follows:

1. **CXX Interface (bridge.rs)**:
   - Defines the FFI interface using the `cxx::bridge` macro
   - Declares C++ types and functions that will be accessed from Rust
   - Special considerations in the interface:
     - Static C++ methods are wrapped as free functions in bridge.h
     - String returns use std::unique_ptr to bridge the language boundary

2. **C++ Adapter (bridge.h)**:
   - Contains helper functions to work around CXX limitations
   - Provides wrappers for static methods (like `getWalletManager()`)
   - CXX can't handle some C++ types being returned by value (especially `std::string` and `std::vector`), so we wrap them in a pointer:
     - if we know we are the owner of the value we use `std::unique_ptr<T>`
     - if we don't know we use `std::shared_ptr<T>`
     - in the case of `TransactionHistory` we use a raw pointer because we aren't the only owner of the value but also need to mutate it.
       Such uses have to be manually verified!

3. **Rust Wrapper (lib.rs)**:
   - Provides idiomatic Rust interfaces to the C++ code
   - Uses wrapper types (WalletManager, Wallet) with safer interfaces
   - Handles memory management and safety concerns:
     - Raw pointers are never exposed to users of the library
     - Implements `Send` and `Sync` for wrapper types
     - Uses `Pin` for C++ objects that require stable memory addresses

4. **Build Process (build.rs)**:
   - Compiles the Monero C++ code with CMake targeting wallet_api
   - Sets up appropriate include paths and library linking
   - Configures CXX to build the bridge between Rust and C++
   - Links numerous static and dynamic libraries required by Monero

5. **Memory Safety Model**:
   - Raw pointers are wrapped in safe Rust types
   - `unsafe` is only used at the FFI boundary
   - Each use of `unsafe` must have a `// Safety: ...` comment explaining why every required invariant is upheld.
   - Proper deref implementations for wrapper types (`impl Deref for RawWallet { type Target = ffi::Wallet;  fn deref(&self) -> &Self::Target { unsafe { self.inner.as_ref().expect("RawWallet is not null") } } }`)
   - The `OnceLock` pattern ensures WalletManager is a singleton

6. **Adding New Functionality**:
   1. Find the desired function in wallet2_api.h
   2. Add its declaration to the `unsafe extern "C++"` block in bridge.rs
   3. Create a corresponding Rust wrapper method in lib.rs
   4. For functions returning strings or with other CXX limitations, add helper functions in bridge.h

7. **Ensuring Memory Safety**:

- _`Send and Sync`_:
  - Don't `unsafe impl Sync`, it's never needed. Work around limitaions if necessary.
  - You can `unsafe impl Send` for a Rust wrapper type _if_ you verified that the type is safe to be moved around threads:
    - By default, assume that _no type is `Send`_
    - A type is _not `Send`_ if it contains a reference/pointer to a non-`Send` type -- notably, `Wallet` and `WalletManager` are not `Send`
    - A type is _not `Send`_ if it uses thread local storage (`WalletManager` and `Wallet`)
    - A type _is `Send`_ if it contains only basic types: integers, strings, vectors of other basic types or other `Send` types.
    - Never implement `Send` unless you have specifically verified that the type actually is.
  - Even if it would be possible, try to avoid implementing `Send` if possible. For example, instead of implementing `unsafe impl Send for TransactionInfoHandle` we immediately copy the values it conaints into our own, Rust-native `TransactionInfo` struct.

This architecture ensures memory safety while providing idiomatic access to the Monero wallet functionality from Rust.
