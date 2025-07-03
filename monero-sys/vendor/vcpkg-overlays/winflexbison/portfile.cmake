# Download pre-built binaries instead of building from source
vcpkg_download_distfile(ARCHIVE
    URLS "https://github.com/lexxmark/winflexbison/releases/download/v2.5.25/win_flex_bison-2.5.25.zip"
    FILENAME "win_flex_bison-2.5.25.zip"
    SHA512 2a829eb05003178c89f891dd0a67add360c112e74821ff28e38feb61dac5b66e9d3d5636ff9eef055616aaf282ee8d6be9f14c6ae4577f60bdcec96cec9f364e
)

# Extract the archive
vcpkg_extract_source_archive_ex(
    OUT_SOURCE_PATH SOURCE_PATH
    ARCHIVE ${ARCHIVE}
    NO_REMOVE_ONE_LEVEL
)

# Install tools to the tools directory
file(INSTALL 
    "${SOURCE_PATH}/win_flex.exe"
    "${SOURCE_PATH}/win_bison.exe"
    DESTINATION "${CURRENT_PACKAGES_DIR}/tools/winflexbison"
)

# Install license (create a simple one if not present)
if(EXISTS "${SOURCE_PATH}/COPYING")
    file(INSTALL "${SOURCE_PATH}/COPYING" DESTINATION "${CURRENT_PACKAGES_DIR}/share/${PORT}" RENAME copyright)
else()
    file(WRITE "${CURRENT_PACKAGES_DIR}/share/${PORT}/copyright" "GPL-3.0-or-later")
endif()

# Make tools available for host
set(VCPKG_POLICY_EMPTY_PACKAGE enabled)