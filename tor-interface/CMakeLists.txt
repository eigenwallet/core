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
# build target
#
add_custom_command(
    DEPENDS ${tor_interface_sources}
    OUTPUT ${tor_interface_outputs}
    COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} cargo build ${CARGO_FLAGS}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR})
add_custom_target(tor_interface_target
    DEPENDS ${tor_interface_outputs})

# cargo test target
add_custom_target(tor_interface_cargo_test_target
    COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test ${CARGO_FLAGS}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
)
add_dependencies(test_target tor_interface_cargo_test_target)

# cargo test (offline) target
add_custom_target(tor_interface_cargo_test_offline_target
    COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test ${CARGO_FLAGS} --features offline-test
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
)
add_dependencies(test_offline_target tor_interface_cargo_test_offline_target)

#
# fuzz target
#
add_custom_target(tor_interface_cargo_fuzz_crypto
    COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo fuzz run fuzz_crypto ${CARGO_FLAGS}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
)
