#!/usr/bin/env bash
# Zips the fixture extension (scripts/vm-ext/krunk-fixture) for the VM run and the tests.
set -eu
cd "$(dirname "$0")"
rm -f krunk-fixture.zip
(cd krunk-fixture && find . -type f | LC_ALL=C sort | zip -X -q -9 ../krunk-fixture.zip -@)
ls -la krunk-fixture.zip
