#!/bin/bash
set -e

CWD=$(dirname "${0}")

_die () {
    echo "ERR: $*" >&2
    exit 1
}

COMPOSE="docker compose"
if ! $COMPOSE >/dev/null; then
    _die "could not call docker compose (hint: install docker compose plugin)"
fi

_help() {
    echo "$name [-h|--help]"
    echo "    show this help message"
    echo
    echo "$name prepare_tests_environment"
    echo "    start and prepare all services"
    echo
    echo "$name prepare_bindings_examples_environment"
    echo "    start and prepare the services required for the bindings examples"
    echo
    echo "$name stop_services"
    echo "    stop services"
    echo
    echo "$name mine <blocks>"
    echo "    mine the requested number of blocks"
    echo
    echo "$name sendtoaddress <address> <amount>"
    echo "    send to a bitcoin address"
    exit 0
}

params=$*
if [ -z "${params}" ]; then
    _help
fi

TMP_DIR="${CWD}/tmp"
LISTS_DIR="${CWD}/lists"
HUB_DIR="${CWD}/hub"
COMPOSE_FPATH="${CWD}/compose.yaml"
COMPOSE="$COMPOSE -f ${COMPOSE_FPATH}"
EXPOSED_PORTS=(3000 50001)  # see compose.yaml for the exposed ports
TIMEOUT=100

BCLI="$COMPOSE exec -T -u blits bitcoind bitcoin-cli -regtest"
BCLI_SIGNET="$COMPOSE exec -T -u blits bitcoind_signet_custom bitcoin-cli -signet"
BCLI_ESPLORA="$COMPOSE exec -T esplora cli"

_prepare_bitcoin_funds() {
    $BCLI createwallet miner
    mine 111
    if [ -n "$TESTS" ]; then
        $BCLI_ESPLORA createwallet miner
        # connect the 2 bitcoind services
        $BCLI addnode "esplora:18444" "add"
        $BCLI_ESPLORA addnode "bitcoind:18444" "add"
        $BCLI_SIGNET createwallet miner
        $BCLI_SIGNET -rpcwallet=miner generatetoaddress 1 "$($BCLI_SIGNET getnewaddress)" 100000000
    fi
}

_wait_for_bitcoind() {
    # wait for bitcoind to be up
    bitcoind_service_name="$1"
    start_time=$(date +%s)
    until $COMPOSE logs $bitcoind_service_name |grep -q 'Bound to'; do
        current_time=$(date +%s)
        if [ $((current_time - start_time)) -gt $TIMEOUT ]; then
            echo "Timeout waiting for $bitcoind_service_name to start"
            $COMPOSE logs $bitcoind_service_name
            exit 1
        fi
        sleep 1
    done
}

_wait_for_electrs() {
    # wait for electrs to have completed startup
    electrs_service_name="$1"
    start_time=$(date +%s)
    until $COMPOSE logs $electrs_service_name |grep -q 'finished full compaction'; do
        current_time=$(date +%s)
        if [ $((current_time - start_time)) -gt $TIMEOUT ]; then
            echo "Timeout waiting for $electrs_service_name to start"
            $COMPOSE logs $electrs_service_name
            exit 1
        fi
        sleep 1
    done
}

_wait_for_esplora() {
    # wait for esplora to have completed startup
    esplora_service_name="$1"
    start_time=$(date +%s)
    until $COMPOSE logs $esplora_service_name |grep -q 'run: nginx:'; do
        current_time=$(date +%s)
        if [ $((current_time - start_time)) -gt $TIMEOUT ]; then
            echo "Timeout waiting for $esplora_service_name to start"
            $COMPOSE logs $esplora_service_name
            exit 1
        fi
        sleep 1
    done
}

_wait_for_proxy() {
    # wait for proxy to have completed startup
    proxy_service_name="$1"
    start_time=$(date +%s)
    until $COMPOSE logs $proxy_service_name |grep -q 'App is running at http://localhost:3000'; do
        current_time=$(date +%s)
        if [ $((current_time - start_time)) -gt $TIMEOUT ]; then
            echo "Timeout waiting for $proxy_service_name to start"
            $COMPOSE logs $proxy_service_name
            exit 1
        fi
        sleep 1
    done
}

stop_services() {
    # cleanly stop the version 0.1.0 RGB proxy server
    local proxy_mod_proto
    proxy_mod_proto="$($COMPOSE ps -q proxy-mod-proto)"
    if [ -n "$proxy_mod_proto" ] && docker ps -q --no-trunc | grep -q "$proxy_mod_proto"; then
        $COMPOSE exec proxy-mod-proto pkill -f '^node'
    fi
    # bring all services down
    $COMPOSE --profile '*' down -v --remove-orphans
}

_start_services() {
    stop_services
    if [ -n "$TESTS" ]; then
        rm -rf $TMP_DIR $LISTS_DIR
        mkdir -p $TMP_DIR $LISTS_DIR
    fi
    for port in "${EXPOSED_PORTS[@]}"; do
        if [ -n "$(ss -HOlnt "sport = :$port")" ];then
            _die "port $port is already bound, services can't be started"
        fi
    done
    $COMPOSE up -d
}

prepare_tests_environment() {
    TESTS=1

    COMPOSE="$COMPOSE --profile tests"
    EXPOSED_PORTS+=(3001 3002 50002 50003 50004 50005 8094 8140 8141)

    PROXY_MOD_PROTO="proxy-mod-proto"
    PROXY_MOD_API="proxy-mod-api"

    # build tests extra services (modified docker images)
    $COMPOSE build $PROXY_MOD_PROTO
    $COMPOSE build $PROXY_MOD_API
    $COMPOSE build esplora

    mkdir -p "${HUB_DIR}"
    touch "${HUB_DIR}/config.toml"

    _start_services

    _wait_for_bitcoind bitcoind

    _wait_for_bitcoind bitcoind_signet_custom

    _prepare_bitcoin_funds

    _wait_for_electrs electrs

    _wait_for_electrs electrs-2

    _wait_for_electrs electrs-blockstream

    _wait_for_electrs electrs_signet_custom

    _wait_for_esplora esplora

    _wait_for_proxy proxy

    _wait_for_proxy $PROXY_MOD_PROTO

    _wait_for_proxy $PROXY_MOD_API
}

prepare_bindings_examples_environment() {
    _start_services

    _wait_for_bitcoind bitcoind

    _prepare_bitcoin_funds

    _wait_for_electrs electrs

    _wait_for_proxy proxy
}

mine() {
    [ -n "$1" ] || _die "num blocks is required"
    $BCLI -rpcwallet=miner -generate "$1" >/dev/null
}

sendtoaddress() {
    [ -n "$1" ] || _die "address is required"
    [ -n "$2" ] || _die "amount is required"
    $BCLI sendtoaddress "$1" "$2"
}

case $1 in
    -h|--help)
        _help
        ;;
    prepare_tests_environment | prepare_bindings_examples_environment | stop_services | mine | sendtoaddress)
        "$@"
        ;;
    *)
        _die "unknown method \"$1\""
        ;;
esac
