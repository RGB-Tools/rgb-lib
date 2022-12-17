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
TEST_DIR="./tests/tmp"

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
