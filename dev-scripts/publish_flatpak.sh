#!/bin/bash

# eigenwallet Flatpak Build and Deploy Script
# Usage: ./flatpak-build.sh [--push] [--branch BRANCH] [--no-gpg]
# Example: ./flatpak-build.sh --push --branch gh-pages

set -e

PUSH_FLAG=""
BRANCH="gh-pages"
GPG_SIGN=""
NO_GPG_FLAG=""
REPO_DIR="flatpak-repo"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --push)
            PUSH_FLAG="--push"
            shift
            ;;
        --branch)
            BRANCH="$2"
            shift 2
            ;;
        --no-gpg)
            NO_GPG_FLAG="--no-gpg"
            shift
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--push] [--branch BRANCH] [--no-gpg]"
            exit 1
            ;;
    esac
done

# Function to list available GPG keys
list_gpg_keys() {
    echo "📋  Available GPG keys:"
    gpg --list-secret-keys --keyid-format=long 2>/dev/null | while read -r type key_info name; do
        if [[ $type = sec ]]; then
            echo "   🔑  Key: $key_info"
        elif [[ $type = uid ]]; then
            echo "      👤  $name"
            echo ""
        fi
    done
}

# Function to get GPG key selection
select_gpg_key() {
    if ! command -v gpg &> /dev/null; then
        echo "❌  GPG is not installed. Install with: sudo apt install gnupg"
        exit 1
    fi

    local keys=($(gpg --list-secret-keys --keyid-format=long 2>/dev/null | awk -F "[$IFS/]*" '/^sec/ {print $3}'))

    if [ ${#keys[@]} -eq 0 ]; then
        echo "🔑  No GPG keys found."
        echo ""
        read -p "Would you like to import a GPG key? [y/N]: " import_key

        if [[ $import_key =~ ^[Yy]$ ]]; then
            import_gpg_key
            select_gpg_key
        else
            echo "⚠️   Proceeding without GPG signing (not recommended for production)"
            return
        fi
    else
        echo ""
        list_gpg_keys

        echo "Please select a GPG key for signing:"
        for i in "${!keys[@]}"; do
            local key_id="${keys[i]}"
            local user_info=$(gpg --list-secret-keys --keyid-format=long "$key_id" 2>/dev/null | awk '/^uid/ {$1=""; $2="\b"; print; exit}')
            echo "   $((i+1))) ${key_id} - ${user_info}"
        done
        echo "   $((${#keys[@]}+1))) Skip GPG signing"
        echo "   $((${#keys[@]}+2))) Import a new key"
        echo ""

        while true; do
            read -p "Enter your choice [1-$((${#keys[@]}+2))]: " choice

            if [[ $choice =~ ^[0-9]+$ ]] && [ $choice -ge 1 ] && [ $choice -le $((${#keys[@]}+2)) ]; then
                if [ $choice -eq $((${#keys[@]}+1)) ]; then
                    echo "⚠️   Proceeding without GPG signing"
                    break
                elif [ $choice -eq $((${#keys[@]}+2)) ]; then
                    import_gpg_key
                    select_gpg_key
                    break
                else
                    GPG_SIGN="${keys[$((choice-1))]}"
                    local selected_user=$(gpg --list-secret-keys --keyid-format=long "$GPG_SIGN" 2>/dev/null | awk '/^uid/ {$1=""; $2="\b"; print; exit}')
                    echo "✅  Selected key: $GPG_SIGN - $selected_user"
                    break
                fi
            else
                echo "❌  Invalid choice. Please enter a number between 1 and $((${#keys[@]}+2))"
            fi
        done
    fi
}

# Function to import GPG key
import_gpg_key() {
    echo ""
    echo "🔑  GPG Key Import"
    echo "=================="
    echo "📝  Please paste your GPG private key below."
    echo "   (Start with -----BEGIN PGP PRIVATE KEY BLOCK----- and end with -----END PGP PRIVATE KEY BLOCK-----)"
    echo "   Press Ctrl+D when finished:"
    echo ""

    if gpg --import - 2>/dev/null; then
        echo "✅  GPG key imported successfully!"
    else
        echo "❌  Failed to import GPG key. Please check the format and try again."
        exit 1
    fi
}

# Check requirements
if ! command -v flatpak-builder &> /dev/null; then
    echo "❌  flatpak-builder is required but not installed"
    echo "Install with: sudo apt install flatpak-builder (Ubuntu/Debian)"
    echo "              sudo dnf install flatpak-builder (Fedora)"
    exit 1
fi

if ! command -v git &> /dev/null; then
    echo "❌  git is required but not installed"
    exit 1
fi

if ! command -v jq &> /dev/null; then
    echo "❌  jq is required but not installed"
    echo "Install with: sudo apt install jq (Ubuntu/Debian)"
    echo "              sudo dnf install jq (Fedora)"
    exit 1
fi

# Get repository info
REPO_URL=$(git remote get-url origin 2>/dev/null || :)
if [[ $REPO_URL =~ github\.com[:/]([^/]+)/([^/.]+) ]]; then
    GITHUB_USER="${BASH_REMATCH[1]}"
    REPO_NAME="${BASH_REMATCH[2]}"
else
    echo "❌  Could not determine GitHub repository info"
    echo "Make sure you're in a Git repository with a GitHub origin"
    exit 1
fi

PAGES_URL="https://${GITHUB_USER}.github.io/${REPO_NAME}"

echo "🏗️   Building Flatpak for eigenwallet..."
echo "📁  Repository: ${GITHUB_USER}/${REPO_NAME}"
echo "🌐  Pages URL: ${PAGES_URL}"
echo ""

# Handle GPG key selection
if [ "$NO_GPG_FLAG" != "--no-gpg" ]; then
    echo "🔐  GPG Signing Setup"
    echo "==================="
    echo "For security, it's highly recommended to sign your Flatpak repository with GPG."
    echo "This ensures users can verify the authenticity of your packages."
    echo ""

    read -p "Do you want to use GPG signing? [Y/n]: " use_gpg

    if [[ $use_gpg =~ ^[Nn]$ ]]; then
        echo "⚠️   Proceeding without GPG signing"
        GPG_SIGN=""
    else
        select_gpg_key
    fi
else
    echo "⚠️   GPG signing disabled by --no-gpg flag"
    GPG_SIGN=""
fi

echo ""

# Always use local .deb file - build if needed
echo "🔍  Ensuring local .deb file exists..."
MANIFEST_FILE="flatpak/org.eigenwallet.app.json"
trap 'rm -f "$TEMP_MANIFEST"' EXIT INT
TEMP_MANIFEST=$(mktemp --suffix=.json)

# Look for the .deb file in the expected location
DEB_FILE=$(find "$PWD/target/debug/bundle/deb/" -name "*.deb" -print -quit)

if [ -f "$DEB_FILE" ]; then
    echo "✅  Found local .deb file: $DEB_FILE"
else
    echo "🏗️   No local .deb file found, building locally..."

    if [ ! -f "./release-build.sh" ]; then
        echo "❌  release-build.sh not found"
        exit 1
    fi

    # Extract version from Cargo.toml
    VERSION=$(awk -F "[$IFS\"]*"  '/^version/ { print $3; exit; }' Cargo.toml)
    if [ -z "$VERSION" ]; then
        echo "❌  Could not determine version from Cargo.toml"
        exit 1
    fi

    echo "📦  Building version $VERSION..."
    ./release-build.sh "$VERSION"

    # Look for the .deb file again
    DEB_FILE=$(find "$PWD/target/debug/bundle/deb/" -name "*.deb" -print -quit)
    if ! [ -f "$DEB_FILE" ]; then
        echo "❌  Failed to build .deb file"
        exit 1
    fi

    echo "✅  Local build completed: $DEB_FILE"
fi

echo "📝  Creating manifest with local .deb..."

# Modify the manifest to use the local file
jq --arg deb_path "$DEB_FILE" --arg PWD "$PWD" '
    .modules[0].sources[0] = {
        "type": "file",
        "path": $deb_path
    } |
    .modules[0].sources[1].path = $PWD + "/" + .modules[0].sources[1].path
' "$MANIFEST_FILE" > "$TEMP_MANIFEST"

MANIFEST_FILE="$TEMP_MANIFEST"
echo "📦  Using local build: ${DEB_FILE##*/}"

echo ""

# Create build directory
rm -rf "$REPO_DIR"
mkdir -p "$REPO_DIR"

# Build arguments
BUILD_ARGS=(
    "build-dir"
    "--user"
    "--install-deps-from=flathub"
    "--disable-rofiles-fuse"
    "--disable-updates"
    "--force-clean"
    "--repo=$REPO_DIR"
)

if [ -n "$GPG_SIGN" ]; then
    BUILD_ARGS+=("--gpg-sign=$GPG_SIGN")
    echo "🔐  GPG signing enabled with key: $GPG_SIGN"
fi

# Add Flathub repository for dependencies
echo "📦  Setting up Flathub repository..."
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo

# Build the Flatpak
echo "🔨  Building Flatpak..."
flatpak-builder "${BUILD_ARGS[@]}" "$MANIFEST_FILE"

# Generate static deltas for faster downloads
echo "⚡  Generating static deltas..."
DELTA_ARGS=("--generate-static-deltas" "--prune")
if [ -n "$GPG_SIGN" ]; then
    DELTA_ARGS+=("--gpg-sign=$GPG_SIGN")
fi
flatpak build-update-repo "${DELTA_ARGS[@]}" "$REPO_DIR"

# Create bundle for direct download
echo "📦  Creating Flatpak bundle..."
BUNDLE_ARGS=("$REPO_DIR" "org.eigenwallet.app.flatpak")
if [ -n "$GPG_SIGN" ]; then
    BUNDLE_ARGS+=("--gpg-sign=$GPG_SIGN")
fi
flatpak build-bundle "${BUNDLE_ARGS[@]}" org.eigenwallet.app

# Add GPG key only if signing
if [ -n "$GPG_SIGN" ]; then
    echo "🔑  Adding GPG keys to .flatpakrepo and .flatpakref..."
    GPGKey="s|%GPGKey%|$(gpg --export "$GPG_SIGN" | base64 -w 0)|"
else
    GPGKey="/%GPGKey%/d"
fi

cp -v flatpak/*.flatpakre* "$REPO_DIR/"
sed -e "s|%Url%|${PAGES_URL}|" \
    -e "s|%Homepage%|https://github.com/${GITHUB_USER}/${REPO_NAME}|" \
    -e "$GPGKey" \
    -i "$REPO_DIR"/*.flatpakre*

# Copy bundle to repo directory
cp org.eigenwallet.app.flatpak "$REPO_DIR/"

# Use index.html from flatpak directory
cp -v flatpak/index.html "$REPO_DIR/"

# Copy any additional files
if [ -f "icon.png" ]; then
    cp icon.png "$REPO_DIR/"
fi

if [ -f "README.md" ]; then
    cp README.md "$REPO_DIR/"
fi

# Add .nojekyll file to skip Jekyll processing
>> "$REPO_DIR/.nojekyll"

echo "✅  Flatpak repository built successfully!"
echo "📊  Repository size: $(du -sh "$REPO_DIR" | { read -r s _; echo "$s"; })"
echo "📁  Repository files are in: $REPO_DIR/"

if [ "$PUSH_FLAG" = "--push" ]; then
    echo ""
    echo "🚀  Deploying to GitHub Pages..."

    # Initialize fresh git repo in deploy directory
    git -C "$REPO_DIR" init
    git -C "$REPO_DIR" add .
    git -C "$REPO_DIR" commit -m "Update Flatpak repository $(date -u '+%F %T %Z')"

    # Push to GitHub Pages branch
    echo "🚀  Force pushing to $BRANCH..."
    git -C "$REPO_DIR" push --force "$REPO_URL" HEAD:"$BRANCH"

    # Clean up
    rm -rf "$REPO_DIR/.git"

    echo "🎉  Deployed successfully!"
    echo "🌐  Your Flatpak repository is available at: $PAGES_URL"
    echo ""
    echo "📋  Users can install with:"
    echo "   flatpak remote-add --user eigenwallet $PAGES_URL/eigenwallet.flatpakrepo"
    echo "   flatpak install eigenwallet org.eigenwallet.app"
    echo ""
    if [ -n "$GPG_SIGN" ]; then
        echo "🔐  Repository is signed with GPG key: $GPG_SIGN"
    fi
else
    echo ""
    echo "📋  To deploy to GitHub Pages, run:"
    echo "   $0 --push"
    echo ""
    echo "📋  Or manually copy the contents of $REPO_DIR/ to your gh-pages branch"
fi
