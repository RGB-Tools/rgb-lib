#!/bin/bash
set -eu

if which docker-compose >/dev/null; then
    COMPOSE_CMD="docker-compose"
elif docker compose >/dev/null; then
    COMPOSE_CMD="docker compose"
else
    echo "could not locate docker compose command or plugin"
    exit 1
fi
COMPOSE="$COMPOSE_CMD -f tests/docker-compose.yml"
PROXY_MOD_PROTO="proxy-mod-proto"
PROXY_MOD_API="proxy-mod-api"
TEST_DIR="./tests/tmp"

$COMPOSE build $PROXY_MOD_PROTO
$COMPOSE build $PROXY_MOD_API

$COMPOSE down -v
rm -rf $TEST_DIR
mkdir -p $TEST_DIR
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
