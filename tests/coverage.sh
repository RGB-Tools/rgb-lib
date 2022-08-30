#!/bin/bash -e
#
# script to run projects tests and report code coverage
#
# uses tarpaulin (https://crates.io/crates/cargo-tarpaulin)
#
# other coverage solutions exist but all require rust nightly and the project
# does not build with nightly at the moment

# install tarpaulin if missing
cargo tarpaulin --help >/dev/null 2>&1 || cargo install cargo-tarpaulin

# run tests
# --skip-clean to avoid re-building everything each time
cargo tarpaulin \
    --count \
    --line \
    --locked \
    --skip-clean \
    --ignore-tests \
    --exclude-files rgb-lib-ffi/ \
    --exclude-files tests/ \
    --exclude-files src/wallet/test/ \
    --out Html \
        -- \
        --test-threads=1

# open the html test report in the default browser
xdg-open tarpaulin-report.html
