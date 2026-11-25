#!/usr/bin/env bash
# release SEMVER_PORTION: _status_clean build integration_test
set -euxo pipefail

# cargo bump {{SEMVER_PORTION}}
cargo bump {{SEMVER_PORTION}}
cargo update -p blunderbuss

VERSION=$(grep '^version = "' Cargo.toml)
[[ $VERSION =~ ([0-9]+\.[0-9]+\.[0-9]+) ]]
VERSION="${BASH_REMATCH[1]}"
echo $VERSION
GIT_VERSION="v$VERSION"
echo $GIT_VERSION

docker tag blainehansen/postgres_migrator blainehansen/postgres_migrator:$VERSION
docker push blainehansen/postgres_migrator:$VERSION
docker push blainehansen/postgres_migrator:latest
git commit -am $GIT_VERSION
git tag $GIT_VERSION
cargo publish

git push origin main
git push origin main --tags
