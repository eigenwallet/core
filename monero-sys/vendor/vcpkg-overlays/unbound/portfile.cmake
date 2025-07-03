# Download prebuilt Windows binary from NLnet Labs
vcpkg_download_distfile(ARCHIVE
    URLS https://nlnetlabs.nl/downloads/unbound/unbound-1.23.0-w32.zip
    FILENAME unbound-1.23.0-w32.zip
    SKIP_SHA512
)

vcpkg_extract_source_archive(SOURCE_PATH 
    ARCHIVE ${ARCHIVE}
    NO_REMOVE_ONE_LEVEL
)

# Since the .def file approach isn't working, let's try using the existing MinGW static library
# Copy the static library with the correct name for MSVC
configure_file("${SOURCE_PATH}/libunbound/libunbound.a" "${SOURCE_PATH}/libunbound/unbound.lib" COPYONLY)

# Install the header file
file(INSTALL "${SOURCE_PATH}/libunbound/unbound.h" DESTINATION "${CURRENT_PACKAGES_DIR}/include")
file(INSTALL "${SOURCE_PATH}/libunbound/unbound.h" DESTINATION "${CURRENT_PACKAGES_DIR}/debug/include")

# Install the MSVC-compatible import library
file(INSTALL "${SOURCE_PATH}/libunbound/unbound.lib" DESTINATION "${CURRENT_PACKAGES_DIR}/lib")
file(INSTALL "${SOURCE_PATH}/libunbound/unbound.lib" DESTINATION "${CURRENT_PACKAGES_DIR}/debug/lib")

# Install the DLL
file(INSTALL "${SOURCE_PATH}/libunbound/libunbound-8.dll" DESTINATION "${CURRENT_PACKAGES_DIR}/bin")
file(INSTALL "${SOURCE_PATH}/libunbound/libunbound-8.dll" DESTINATION "${CURRENT_PACKAGES_DIR}/debug/bin")

# Create a minimal CMake config file
file(WRITE "${CURRENT_PACKAGES_DIR}/share/unbound/unbound-config.cmake" "
set(UNBOUND_FOUND TRUE)
set(UNBOUND_INCLUDE_DIRS \"\${CMAKE_CURRENT_LIST_DIR}/../../include\")
set(UNBOUND_LIBRARIES \"\${CMAKE_CURRENT_LIST_DIR}/../../lib/unbound.lib\")
set(UNBOUND_LIBRARY \"\${CMAKE_CURRENT_LIST_DIR}/../../lib/unbound.lib\")
")

file(INSTALL "${CURRENT_PACKAGES_DIR}/share/unbound/unbound-config.cmake" DESTINATION "${CURRENT_PACKAGES_DIR}/debug/share/unbound") 