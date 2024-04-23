#!/bin/bash
set -eu

_die () {
    echo "ERR: $*"
    exit 1
}

COMPOSE="docker compose"
if ! $COMPOSE >/dev/null; then
    echo "could not call docker compose (hint: install docker compose plugin)"
    exit 1
fi
COMPOSE="$COMPOSE -f tests/docker-compose.yml"
BCLI="$COMPOSE exec -T -u blits bitcoind bitcoin-cli -regtest"
BCLI_ESPLORA="$COMPOSE exec -T esplora cli"
PROXY_MOD_PROTO="proxy-mod-proto"
PROXY_MOD_API="proxy-mod-api"
TEST_DIR="./tests/tmp"

# build modified docker images
$COMPOSE build $PROXY_MOD_PROTO
$COMPOSE build $PROXY_MOD_API

# restart services (down + up) checking for ports availability
$COMPOSE down -v
rm -rf $TEST_DIR
mkdir -p $TEST_DIR
# see docker-compose.yml for the exposed ports
EXPOSED_PORTS=(3000 3001 3002 8094 50001 50002 50003 50004)
for port in "${EXPOSED_PORTS[@]}"; do
    if [ -n "$(ss -HOlnt "sport = :$port")" ];then
        _die "port $port is already bound, services can't be started"
    fi
done
$COMPOSE up -d

# wait for bitcoind to be up
until $COMPOSE logs bitcoind |grep 'Bound to'; do
    sleep 1
done

# prepare bitcoin funds
$BCLI createwallet miner
$BCLI -rpcwallet=miner -generate 111
$BCLI_ESPLORA createwallet miner

# wait for electrs to have completed startup
until $COMPOSE logs electrs |grep 'finished full compaction'; do
    sleep 1
done
until $COMPOSE logs electrs-2 |grep 'finished full compaction'; do
    sleep 1
done
until $COMPOSE logs electrs-blockstream |grep 'finished full compaction'; do
    sleep 1
done

# wait for proxy to have completed startup
until $COMPOSE logs proxy |grep 'App is running at http://localhost:3000'; do
    sleep 1
done

# wait for modified proxies to have completed startup
until $COMPOSE logs $PROXY_MOD_PROTO |grep 'App is running at http://localhost:3000'; do
    sleep 1
done
until $COMPOSE logs $PROXY_MOD_API |grep 'App is running at http://localhost:3000'; do
    sleep 1
done

# wait for esplora to have completed setup
until $COMPOSE logs esplora |grep -q 'Bootstrapped 100%'; do
    sleep 1
done

# connect the 2 bitcoind services
$BCLI addnode "esplora:18444" "onetry"
$BCLI_ESPLORA addnode "bitcoind:18444" "onetry"
