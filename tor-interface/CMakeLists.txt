set(tor_interface_sources
    Cargo.toml
    src/legacy_tor_client.rs
    src/legacy_tor_controller.rs
    src/legacy_tor_control_stream.rs
    src/legacy_tor_process.rs
    src/legacy_tor_version.rs
    src/lib.rs
    src/mock_tor_client.rs
    src/tor_crypto.rs
    src/tor_provider.rs)

set(tor_interface_outputs
    ${CARGO_TARGET_DIR}/${CARGO_PROFILE}/libtor_interface.d
    ${CARGO_TARGET_DIR}/${CARGO_PROFILE}/libtor_interface.rlib)

#
# tor-interface crate feature flags
#

set(TOR_INTERFACE_FEATURE_LIST)
if (ENABLE_LEGACY_TOR_PROVIDER)
    list(APPEND TOR_INTERFACE_FEATURE_LIST "legacy-tor-provider")
endif()

list(JOIN TOR_INTERFACE_FEATURE_LIST "," TOR_INTERFACE_FEATURES)
if (TOR_INTERFACE_FEATURES)
    set(TOR_INTERFACE_FEATURES "--features" "${TOR_INTERFACE_FEATURES}")
endif()

#
# build target
#
add_custom_command(
    DEPENDS ${tor_interface_sources}
    OUTPUT ${tor_interface_outputs}
    COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} cargo build ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR})

add_custom_target(tor_interface_target
    DEPENDS ${tor_interface_outputs})

#
# cargo test target
#
if (ENABLE_TESTS)
    add_test(NAME tor_interface_cargo_test
        COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES}
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    )
endif()

#
# fuzz target
#
if (ENABLE_FUZZ_TESTS)
    add_test(NAME tor_interface_crypto_cargo_fuzz_test
        COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo fuzz run fuzz_crypto -- -max_total_time=${FUZZ_TEST_MAX_TOTAL_TIME}
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    )
endif()
