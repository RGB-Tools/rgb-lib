#!/bin/bash

COMPOSE="docker-compose -f tests/docker-compose.yml"
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
$BCLI -rpcwallet=miner -generate 103

# wait for electrs to have completed startup
until $COMPOSE logs electrs |grep 'finished full compaction'; do
    sleep 1
done
