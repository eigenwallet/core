#!/bin/bash

# regenerate_sqlx_cache.sh
#
# Script to regenerate SQLx query cache for monero-rpc-pool
#
# This script:
# 1. Creates a temporary SQLite database in the workspace root
# 2. Runs all database migrations to set up the schema
# 3. Regenerates the SQLx query cache (.sqlx directory)
# 4. Cleans up temporary database file automatically
#
# Usage:
#   ./regenerate_sqlx_cache.sh
#
# Requirements:
# - cargo and sqlx-cli must be installed
# - Must be run from the monero-rpc-pool directory
# - migrations/ directory must exist with valid migration files
#
# The generated .sqlx directory should be committed to version control
# to enable offline compilation without requiring DATABASE_URL.

set -e  # Exit on any error

# Ensure sqlx-cli is installed and is at least version 0.8
if ! command -v cargo-sqlx &> /dev/null; then
    echo "❌ sqlx-cli is not installed. Install it with: cargo install sqlx-cli --version 0.8.6 --features sqlite --no-default-features"
    exit 1
fi

SQLX_VERSION=$(cargo sqlx --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
SQLX_MAJOR=$(echo "$SQLX_VERSION" | cut -d. -f1)
SQLX_MINOR=$(echo "$SQLX_VERSION" | cut -d. -f2)

if [ "$SQLX_MAJOR" -lt 1 ] && [ "$SQLX_MINOR" -lt 8 ]; then
    echo "❌ sqlx-cli version $SQLX_VERSION is too old. Version 0.8+ is required (--workspace flag)."
    echo "   Upgrade with: cargo install sqlx-cli --version 0.8.6 --features sqlite --no-default-features"
    exit 1
fi

echo "🔄 Regenerating SQLx query cache..."

WORKSPACE_ROOT="$PWD"

# Use shared temporary database in workspace root
TEMP_DB="$WORKSPACE_ROOT/tempdb.sqlite"
DATABASE_URL="sqlite:$TEMP_DB"

rm -f "$TEMP_DB"

echo "📁 Using temporary database: $TEMP_DB"

# Export DATABASE_URL for sqlx commands
export DATABASE_URL

echo "🗄️  Creating database..."
cargo sqlx database create

for dir in swap monero-sys monero-rpc-pool; do
    echo "🔄 Running migrations in $dir..."
    (cd "$WORKSPACE_ROOT/$dir" && rm -rf .sqlx && cargo sqlx migrate run --ignore-missing)
done

echo "⚡ Preparing SQLx query cache..."
cargo sqlx prepare --workspace

echo "✅ SQLx query cache regenerated successfully!"
echo "📝 The .sqlx directory has been updated with the latest query metadata."
echo "💡 Make sure to commit the .sqlx directory to version control."
