#!/bin/bash
set -eu

COMPOSE="docker compose"
if ! $COMPOSE >/dev/null; then
    echo "could not call docker compose (hint: install docker compose plugin)"
    exit 1
fi
COMPOSE="$COMPOSE -f tests/docker-compose.yml"
PROXY_MOD_PROTO="proxy-mod-proto"
PROXY_MOD_API="proxy-mod-api"
TEST_DIR="./tests/tmp"

$COMPOSE build $PROXY_MOD_PROTO
$COMPOSE build $PROXY_MOD_API

$COMPOSE down -v
rm -rf $TEST_DIR
mkdir -p $TEST_DIR
# see docker-compose.yml for the exposed ports
EXPOSED_PORTS=(3000 3001 3002 50001 50002)
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
BCLI="$COMPOSE exec -T -u blits bitcoind bitcoin-cli -regtest"
$BCLI createwallet miner
$BCLI -rpcwallet=miner -generate 111

# wait for electrs to have completed startup
until $COMPOSE logs electrs |grep 'finished full compaction'; do
    sleep 1
done

# wait for proxy to have completed startup
until $COMPOSE logs proxy |grep 'App is running at http://localhost:3000'; do
    sleep 1
done

# wait for modified proxiesto have completed startup
until $COMPOSE logs $PROXY_MOD_PROTO |grep 'App is running at http://localhost:3000'; do
    sleep 1
done
until $COMPOSE logs $PROXY_MOD_API |grep 'App is running at http://localhost:3000'; do
    sleep 1
done
