vcpkg_from_github(
    OUT_SOURCE_PATH SOURCE_PATH
    REPO lexxmark/winflexbison
    REF v2.5.25
    SHA512 0
    HEAD_REF master
)

# Use CMake build
vcpkg_configure_cmake(
    SOURCE_PATH ${SOURCE_PATH}
    PREFER_NINJA
)

vcpkg_install_cmake()

# Install tools to the tools directory
file(INSTALL 
    "${CURRENT_BUILDTREES_DIR}/${TARGET_TRIPLET}-rel/win_flex.exe"
    "${CURRENT_BUILDTREES_DIR}/${TARGET_TRIPLET}-rel/win_bison.exe"
    DESTINATION "${CURRENT_PACKAGES_DIR}/tools/winflexbison"
)

# Install copyright file
file(INSTALL "${SOURCE_PATH}/COPYING" DESTINATION "${CURRENT_PACKAGES_DIR}/share/${PORT}" RENAME copyright)

# Make tools available for host
if(NOT VCPKG_TARGET_IS_WINDOWS OR VCPKG_HOST_IS_WINDOWS)
    set(VCPKG_POLICY_EMPTY_PACKAGE enabled)
endif()