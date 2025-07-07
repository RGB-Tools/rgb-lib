#include "rgblib.h"
#include <json-c/json.h>
#include <stdio.h>

int main() {
    const char *bitcoin_network = "Regtest";
    CResultString keys_res = rgblib_generate_keys(bitcoin_network);
    if (keys_res.result == Err) {
        printf("ERR: %s\n", keys_res.inner);
        return EXIT_FAILURE;
    }
    const char *keys = keys_res.inner;
    printf("Keys: %s\n", keys);

    struct json_object *keys_obj = json_tokener_parse(keys);
    const char *mnemonic =
        json_object_get_string(json_object_object_get(keys_obj, "mnemonic"));
    const char *account_xpub_vanilla = json_object_get_string(
        json_object_object_get(keys_obj, "account_xpub_vanilla"));
    const char *account_xpub_colored = json_object_get_string(
        json_object_object_get(keys_obj, "account_xpub_colored"));
    const char *master_fingerprint = json_object_get_string(
        json_object_object_get(keys_obj, "master_fingerprint"));
    char wallet_data[400];
    sprintf(wallet_data,
            "{ \"data_dir\": \"./data\", \"bitcoin_network\": \"Regtest\", "
            "\"database_type\": \"Sqlite\", \"max_allocations_per_utxo\": "
            "\"1\", \"account_xpub_vanilla\": \"%s\", "
            "\"account_xpub_colored\": \"%s\", \"mnemonic\": \"%s\", "
            "\"master_fingerprint\": \"%s\", \"vanilla_keychain\": null,"
            "\"supported_schemas\": [\"Nia\", \"Cfa\", \"Uda\"] }",
            account_xpub_vanilla, account_xpub_colored, mnemonic,
            master_fingerprint);

    printf("Creating wallet...\n");
    CResult wallet = rgblib_new_wallet(wallet_data);
    if (wallet.result == Err) {
        printf("ERR: %s\n", wallet.inner);
        return EXIT_FAILURE;
    }
    const struct COpaqueStruct *wlt = &wallet.inner;
    printf("Wallet created\n");

    CResultString address_res = rgblib_get_address(wlt);
    if (address_res.result == Err) {
        printf("ERR: %s\n", address_res.inner);
        return EXIT_FAILURE;
    }
    const char *address = address_res.inner;
    printf("Address: %s\n", address);

    char command[100];
    sprintf(command, "../../tests/regtest.sh sendtoaddress %s 1", address);
    int result = system(command);
    if (result == -1) {
        perror("Error executing command\n");
        return EXIT_FAILURE;
    } else if (WIFEXITED(result) && WEXITSTATUS(result) == 0) {
        printf("Sent\n");
    } else {
        printf("Command failed to execute\n");
        return EXIT_FAILURE;
    }

    CResultString btc_balance_res_1 = rgblib_get_btc_balance(wlt, NULL, true);
    if (btc_balance_res_1.result == Err) {
        printf("ERR: %s\n", btc_balance_res_1.inner);
        return EXIT_FAILURE;
    }
    const char *btc_balance_1 = btc_balance_res_1.inner;
    printf("BTC balance: %s\n", btc_balance_1);

    printf("Wallet is going online...\n");
    CResult online_res = rgblib_go_online(wlt, false, "tcp://localhost:50001");
    if (online_res.result == Err) {
        printf("ERR: %s\n", online_res.inner);
        return EXIT_FAILURE;
    }
    const struct COpaqueStruct *online = &online_res.inner;
    printf("Wallet went online\n");

    CResultString btc_balance_res_2 =
        rgblib_get_btc_balance(wlt, online, false);
    if (btc_balance_res_2.result == Err) {
        printf("ERR: %s\n", btc_balance_res_2.inner);
        return EXIT_FAILURE;
    }
    const char *btc_balance_2 = btc_balance_res_2.inner;
    printf("BTC balance after sync: %s\n", btc_balance_2);

    CResultString created_res =
        rgblib_create_utxos(wlt, online, false, "25", NULL, "1", false);
    if (created_res.result == Err) {
        printf("ERR: %s\n", created_res.inner);
        return EXIT_FAILURE;
    }
    const char *created = created_res.inner;
    printf("Created %s UTXOs\n", created);

    CResultString asset_nia_res =
        rgblib_issue_asset_nia(wlt, "USDT", "Tether", "2", "[\"777\", \"66\"]");
    if (asset_nia_res.result == Ok) {
        printf("Issued a NIA asset: %s\n", asset_nia_res.inner);
    } else {
        printf("ERR: %s\n", asset_nia_res.inner);
        return EXIT_FAILURE;
    }

    CResultString asset_cfa_res =
        rgblib_issue_asset_cfa(wlt, "Cfa", "desc", "2", "[\"777\"]", NULL);
    if (asset_cfa_res.result == Ok) {
        printf("Issued a CFA asset: %s\n", asset_cfa_res.inner);
    } else {
        printf("ERR: %s\n", asset_cfa_res.inner);
        return EXIT_FAILURE;
    }

    CResultString asset_uda_res = rgblib_issue_asset_uda(
        wlt, "TKN", "Token", NULL, "2", "README.md", "[]");
    if (asset_uda_res.result == Ok) {
        printf("Issued a UDA asset: %s\n", asset_uda_res.inner);
    } else {
        printf("ERR: %s\n", asset_uda_res.inner);
        return EXIT_FAILURE;
    }

    const char *filter_asset_schemas_1 = "[\"Nia\", \"Cfa\"]";
    CResultString assets_res_1 =
        rgblib_list_assets(wlt, filter_asset_schemas_1);
    if (assets_res_1.result == Err) {
        printf("ERR: %s\n", assets_res_1.inner);
        return EXIT_FAILURE;
    }
    const char *assets_1 = assets_res_1.inner;
    printf("Assets: %s\n", assets_1);

    const char *filter_asset_schemas_2 = "[]";
    CResultString assets_res_2 =
        rgblib_list_assets(wlt, filter_asset_schemas_2);
    if (assets_res_2.result == Err) {
        printf("ERR: %s\n", assets_res_2.inner);
        return EXIT_FAILURE;
    }
    const char *assets_2 = assets_res_2.inner;
    printf("Assets: %s\n", assets_2);

    const char *assignment = "{\"Fungible\":77}";
    const char *transport_endpoints = "[\"rpc://127.0.0.1:3000/json-rpc\"]";
    CResultString receive_data_res = rgblib_blind_receive(
        wlt, NULL, assignment, NULL, transport_endpoints, "1");
    if (receive_data_res.result == Ok) {
        printf("Receive data: %s\n", receive_data_res.inner);
    } else {
        printf("ERR: %s\n", receive_data_res.inner);
        return EXIT_FAILURE;
    }

    CResultString sync_res = rgblib_sync(wlt, online);
    if (sync_res.result == Ok) {
        printf("Synced\n");
    } else {
        printf("ERR: %s\n", sync_res.inner);
        return EXIT_FAILURE;
    }

    CResultString fee_res = rgblib_get_fee_estimation(wlt, online, "7");
    printf("Fee estimation: %s\n", fee_res.inner);

    CResultString transfers_res = rgblib_list_transfers(wlt, NULL);
    if (transfers_res.result == Err) {
        printf("ERR: %s\n", transfers_res.inner);
        return EXIT_FAILURE;
    }
    const char *transfers = transfers_res.inner;
    printf("Transfers: %s\n", transfers);

    return EXIT_SUCCESS;
}
