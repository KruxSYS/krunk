#!/usr/bin/env bash
# Zips the fixture extensions (scripts/vm-ext/krunk-fixture, and since 0.11.7 the audit's
# krunk-attacker) for the VM runs and the tests.
set -eu
cd "$(dirname "$0")"
rm -f krunk-fixture.zip
(cd krunk-fixture && find . -type f | LC_ALL=C sort | zip -X -q -9 ../krunk-fixture.zip -@)
rm -f krunk-attacker.zip
(cd krunk-attacker && find . -type f | LC_ALL=C sort | zip -X -q -9 ../krunk-attacker.zip -@)
ls -la krunk-fixture.zip krunk-attacker.zip
