set(tor_interface_sources
    Cargo.toml
    src/arti_client_tor_client.rs
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
if (ENABLE_MOCK_TOR_PROVIDER)
    list(APPEND TOR_INTERFACE_FEATURE_LIST "mock-tor-provider")
endif()
if (ENABLE_LEGACY_TOR_PROVIDER)
    list(APPEND TOR_INTERFACE_FEATURE_LIST "legacy-tor-provider")
endif()
if (ENABLE_ARTI_CLIENT_TOR_PROVIDER)
    list(APPEND TOR_INTERFACE_FEATURE_LIST "arti-client-tor-provider")
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
    if (ENABLE_MOCK_TOR_PROVIDER)
        add_test(NAME tor_interface_mock_bootstrap_cargo_test
            COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_mock_bootstrap ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        )
        add_test(NAME tor_interface_mock_onion_service_cargo_test
            COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_mock_onion_service ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        )
        add_test(NAME tor_interface_mock_authenticated_onion_service_cargo_test
            COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_mock_authenticated_onion_service ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        )
    endif()

    if (ENABLE_LEGACY_TOR_PROVIDER)
        add_test(NAME tor_interface_legacy_bootstrap_cargo_test
            COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_legacy_bootstrap ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        )
        add_test(NAME tor_interface_legacy_onion_service_cargo_test
            COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_legacy_onion_service ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        )
        add_test(NAME tor_interface_legacy_authenticated_onion_service_cargo_test
            COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_legacy_authenticated_onion_service ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        )
        add_test(NAME tor_interface_system_legacy_onion_service_cargo_test
            COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_system_legacy_onion_service ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        )
        add_test(NAME tor_interface_system_legacy_authenticated_onion_service_cargo_test
            COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_system_legacy_authenticated_onion_service ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        )
    endif()

    if (ENABLE_ARTI_CLIENT_TOR_PROVIDER)
        add_test(NAME tor_interface_arti_client_bootstrap_cargo_test
            COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_arti_client_bootstrap ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        )
        add_test(NAME tor_interface_arti_client_onion_service_cargo_test
            COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_arti_client_onion_service ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        )
    endif()

    if (ENABLE_LEGACY_TOR_PROVIDER AND ENABLE_ARTI_CLIENT_TOR_PROVIDER)
        add_test(NAME tor_interface_mixed_arti_client_legacy_bootstrap_cargo_test
            COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_mixed_arti_client_legacy_onion_service ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        )
        add_test(NAME tor_interface_mixed_legacy_arti_client_bootstrap_cargo_test
            COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_mixed_legacy_arti_client_onion_service ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
            WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        )
    endif()

    # cryptography
    add_test(NAME tor_interface_crypto_cargo_test
        COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_crypto_ ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    )

    # tor provider utils
    add_test(NAME tor_interface_tor_provider_cargo_test
        COMMAND env CARGO_TARGET_DIR=${CARGO_TARGET_DIR} RUSTFLAGS=${RUSTFLAGS} RUST_BACKTRACE=full cargo test test_tor_provider_ ${CARGO_FLAGS} ${TOR_INTERFACE_FEATURES} -- --nocapture
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
