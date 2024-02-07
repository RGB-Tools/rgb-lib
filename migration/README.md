# DB migrations

Every time a change to a DB object or table is needed, a migration has to be
created.

In rgb-lib we use sea-orm tools to handle the DB and its migrations.

To generate new migrations the [sea-orm-cli] tool is needed. You should install
the same version that has been previously used. You can find this in the
`src/database/entities/mod.rs` file, where the first line will specify `//!
SeaORM Entity. Generated by sea-orm-codegen <VERSION>`. Install it with:
```sh
cargo install sea-orm-cli --version <VERSION>
```

Then, to generate a new migration file, run:
```sh
sea-orm-cli migrate generate <migration_name>
```

This command will create a new file where you'll find the `up` and `down`
methods (see `migration/src/m20230608_071249_init_db.rs` for an example). These
methods will be empty and will need to be implemented in order to give
instructions on how to respectively update and revert the new changes.

Once the migration file is ready, you'll need to run a local postgres DB and
use it to refresh the migration and generate entities with `sea-orm-cli`. This
is accomplished with:
```sh
docker pull postgres:latest

docker run -p 127.0.0.1:5432:5432/tcp --name migration-postgres \
    -e POSTGRES_PASSWORD=mysecretpassword -d postgres

DATABASE_URL=postgres://postgres:mysecretpassword@localhost:5432 \
    sea-orm-cli migrate refresh

DATABASE_URL=postgres://postgres:mysecretpassword@localhost:5432/postgres \
    sea-orm-cli generate entity -o src/database/entities --expanded-format

docker rm -f migration-postgres
```

The command to generate entities will apply some unwanted changes, for example
it will change the enum fields to integers and will remove some extra `derive`s
that we manually added. Those changes will need to be discarded, so please be
sure to add only the code that is related to the new changes you just applied.
To do this we suggest to first refresh the migration and generate entities with
`sea-orm-cli` on the branch you are about to apply the DB changes on. The
generated diff will only include unwanted changes, so they can be used as a
reference to revert them.


[sea-orm-cli]: https://github.com/SeaQL/sea-orm/tree/master/sea-orm-cli