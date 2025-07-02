vcpkg_from_github(
    OUT_SOURCE_PATH SOURCE_PATH
    REPO lexxmark/winflexbison
    REF v2.5.25
    SHA512 7a797d5a1aef21786b4ce8bc8f2a31c4957e55012a4d29b14fbe6d89c1b8ad33e7ab6d1afec6b37ddccd1696dc5b861da568fc8a14d22bb33aa7c1116172d7cf
    HEAD_REF master
)

# Use CMake build
vcpkg_cmake_configure(
    SOURCE_PATH "${SOURCE_PATH}"
    OPTIONS
        -DFLEX_VERSION=2.6.4
        -DBISON_VERSION=3.8.2
)

vcpkg_cmake_build()

# Install tools to the tools directory
file(INSTALL 
    "${CURRENT_BUILDTREES_DIR}/${TARGET_TRIPLET}-rel/win_flex.exe"
    "${CURRENT_BUILDTREES_DIR}/${TARGET_TRIPLET}-rel/win_bison.exe"
    DESTINATION "${CURRENT_PACKAGES_DIR}/tools/winflexbison"
)

# Make tools available for host
if(NOT VCPKG_TARGET_IS_WINDOWS OR VCPKG_HOST_IS_WINDOWS)
    set(VCPKG_POLICY_EMPTY_PACKAGE enabled)
endif()

# Install license
vcpkg_install_copyright(FILE_LIST "${SOURCE_PATH}/COPYING")