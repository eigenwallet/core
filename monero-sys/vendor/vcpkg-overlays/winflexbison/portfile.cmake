vcpkg_from_github(
    OUT_SOURCE_PATH SOURCE_PATH
    REPO lexxmark/winflexbison
    REF v2.5.25
    SHA512 18a1a6a0b38b9f44b0f06c0d02aa0a9b6bc65a03ad4c1b6b9e3f1b9e4bc1c3a2b3e4c5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8
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