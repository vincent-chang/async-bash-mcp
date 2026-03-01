#!/usr/bin/env bash
set -euo pipefail

VERSION=${1:?"Usage: ./scripts/release.sh <version>  (e.g. ./scripts/release.sh 0.2.0)"}

# Validate semver format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: version must be in semver format X.Y.Z (got: $VERSION)" >&2
    exit 1
fi

# Update version in Cargo.toml
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml

# Verify the change
FOUND=$(grep '^version = ' Cargo.toml | head -1)
echo "Updated Cargo.toml: $FOUND"

# Commit and tag
git add Cargo.toml
git commit -m "chore: bump version to $VERSION"

git tag -a "v$VERSION" -m "Release version $VERSION"

git push
git push --tags
