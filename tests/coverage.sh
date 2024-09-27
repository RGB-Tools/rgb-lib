#!/bin/bash -e
#
# script to run project tests and report code coverage
# uses llvm-cov (https://github.com/taiki-e/cargo-llvm-cov)

LLVM_COV_OPTS=()
CARGO_TEST_OPTS=("--")
COV="cargo llvm-cov --workspace --all-features"

_die() {
    echo "err $*"
    exit 1
}

_tit() {
    echo
    echo "========================================"
    echo "$@"
    echo "========================================"
}

help() {
    echo "$NAME [-h|--help] [-t|--test] [--ci] [--ignore-run-fail] [--no-clean]"
    echo ""
    echo "options:"
    echo "    -h --help             show this help message"
    echo "    -t --test             only run these test(s)"
    echo "       --ci               run for the CI"
    echo "       --ignore-run-fail  keep running regardless of failure"
    echo "       --no-clean         don't cleanup before the run"
}

# cmdline arguments
while [ -n "$1" ]; do
    case $1 in
        -h|--help)
            help
            exit 0
            ;;
        -t|--test)
            CARGO_TEST_OPTS+=("$2")
            shift
            ;;
        --ci)
            COV_CI="$COV --lcov --output-path coverage.lcov"
            $COV_CI -- --ignored get_fee_estimation::fail_
            SKIP_INIT=1 $COV_CI --no-clean
            exit 0
            ;;
        --ignore-run-fail)
            LLVM_COV_OPTS+=("$1")
            ;;
        --no-clean)
            LLVM_COV_OPTS+=("$1")
            ;;
        *)
            help
            _die "unsupported argument \"$1\""
            ;;
    esac
    shift
done

_tit "installing requirements"
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov

_tit "generating coverage report"
# shellcheck disable=2086
$COV --html "${LLVM_COV_OPTS[@]}" "${CARGO_TEST_OPTS[@]}" --include-ignored

## show html report location
echo "generated html report: target/llvm-cov/html/index.html"
