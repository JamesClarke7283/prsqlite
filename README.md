# prsqlite - Pure Rust SQLite

Pure Rust implementation of [SQLite](https://www.sqlite.org/index.html).

This is WIP and my hobby project.

* Compatible with database file generated by sqlite3
  * prsqlite uses the SQLite
  [Database File Format](https://www.sqlite.org/fileformat2.html#b_tree_pages).
* Support SQL syntax which SQLite supports
  * See [SQL As Understood By SQLite](https://www.sqlite.org/lang.html).
  * Some syntax is not implemented yet.
* Zero dependency
  * except dev-dependency.
  * While developing as WIP, prsqlite is using `anyhow` for development
  velocity. It will be replaced with a proprietary errors in the future.
* Validating file format
  * prsqlite does not trust the file is valid unlike sqlite3 and validates pages
  in the file while parsing.
  * `trust-file` feature will be added to disable the file validation.
* No unsafe
  * Will be supported in the future.

NOTE: This repository is not stable yet. I may force-push commit tree even on
the main branch.

## Usage

See [integration_test.rs](./tests/integration_test.rs) for what prsqlite
supports.

`prsqlite::Connection::open()` is the entrypoint interface for library users.

```rs
use std::path::Path;

use prsqlite::Connection;
use prsqlite::NextRow;
use prsqlite::Value;

let mut conn = Connection::open(Path::new("path/to/sqlite.db")).unwrap();
let mut stmt = conn.prepare("SELECT * FROM example WHERE col = 1;").unwrap();
let mut rows = stmt.execute().unwrap();

let row = rows.next_row().unwrap().unwrap();
let columns = row.parse().unwrap();
assert_eq!(columns.get(0), &Value::Integer(1));
drop(row);

assert!(rows.next().unwrap().is_none());
```

prsqlite provides REPL command.

```bash
$ git clone https://github.com/kawasin73/prsqlite.git

$ cd ./prsqlite

$ sqlite3 tmp/sqlite.db
sqlite> CREATE TABLE example(col1, col2 integer);
sqlite> CREATE INDEX i_example ON example(col2);
sqlite> INSERT INTO example(col1, col2) values(null, 1);
sqlite> INSERT INTO example(col1, col2) values(10, 2);
sqlite> INSERT INTO example(col1, col2) values(1.1, 3);
sqlite> INSERT INTO example(col1, col2) values('Hello prsqlite!', 4);
sqlite> INSERT INTO example(col1, col2) values(X'707273716c697465', 5);
sqlite> .quit

$ cargo build && ./target/debug/prsqlite tmp/sqlite.db
prsqlite> SELECT * FROM sqlite_schema;
table|example|example|2|CREATE TABLE example(col1, col2 integer)
index|i_example|example|3|CREATE INDEX i_example ON example(col2)
prsqlite> SELECT * FROM example;
|1
10|2
1.1|3
Hello prsqlite!|4
prsqlite|5
prsqlite> SELECT col1 FROM example WHERE col2 == 4;
Hello prsqlite!
prsqlite> .quit
```

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for details.

## License

Apache 2.0; see [`LICENSE`](LICENSE) for details.

## Disclaimer

This project is not an official Google project. It is not supported by
Google and Google specifically disclaims all warranties as to its quality,
merchantability, or fitness for a particular purpose.
