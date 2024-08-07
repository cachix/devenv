  # Postgres
  


## services\.postgres\.enable



Whether to enable Add PostgreSQL process\.
\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.postgres\.package



The PostgreSQL package to use\. Use this to override the default with a specific version\.



*Type:*
package



*Default:*
` pkgs.postgresql `



*Example:*

```
pkgs.postgresql_15

```



## services\.postgres\.createDatabase

Create a database named like current user on startup\. Only applies when initialDatabases is an empty list\.



*Type:*
boolean



*Default:*
` true `



## services\.postgres\.extensions



Additional PostgreSQL extensions to install\.

The available extensions are:

 - age
 - anonymizer
 - apache_datasketches
 - citus
 - cstore_fdw
 - h3-pg
 - hypopg
 - jsonb_deep_sum
 - lantern
 - periods
 - pg_auto_failover
 - pg_bigm
 - pg_cron
 - pg_ed25519
 - pg_embedding
 - pg_hint_plan
 - pg_hll
 - pg_ivm
 - pg_libversion
 - pg_net
 - pg_partman
 - pg_rational
 - pg_relusage
 - pg_repack
 - pg_roaringbitmap
 - pg_safeupdate
 - pg_similarity
 - pg_squeeze
 - pg_topn
 - pg_uuidv7
 - pgaudit
 - pgjwt
 - pgroonga
 - pgrouting
 - pgsodium
 - pgsql-http
 - pgtap
 - pgvecto-rs
 - pgvector
 - plpgsql_check
 - plr
 - plv8
 - postgis
 - promscale_extension
 - repmgr
 - rum
 - smlar
 - tds_fdw
 - temporal_tables
 - timescaledb
 - timescaledb-apache
 - timescaledb_toolkit
 - tsearch_extras
 - tsja
 - wal2json



*Type:*
null or (function that evaluates to a(n) list of package)



*Default:*
` null `



*Example:*

```
extensions: [
  extensions.pg_cron
  extensions.postgis
  extensions.timescaledb
];

```



## services\.postgres\.initdbArgs



Additional arguments passed to ` initdb ` during data dir
initialisation\.



*Type:*
list of strings concatenated with “\\n”



*Default:*

```
[
  "--locale=C"
  "--encoding=UTF8"
]
```



*Example:*

```
[
  "--data-checksums"
  "--allow-group-access"
]
```



## services\.postgres\.initialDatabases



List of database names and their initial schemas that should be used to create databases on the first startup
of Postgres\. The schema attribute is optional: If not specified, an empty database is created\.



*Type:*
list of (submodule)



*Default:*
` [ ] `



*Example:*

```
[
  {
    name = "foodatabase";
    schema = ./foodatabase.sql;
  }
  { name = "bardatabase"; }
]

```



## services\.postgres\.initialDatabases\.\*\.name



The name of the database to create\.



*Type:*
string



## services\.postgres\.initialDatabases\.\*\.schema



The initial schema of the database; if null (the default),
an empty database is created\.



*Type:*
null or path



*Default:*
` null `



## services\.postgres\.initialScript



Initial SQL commands to run during database initialization\. This can be multiple
SQL expressions separated by a semi-colon\.



*Type:*
null or string



*Default:*
` null `



*Example:*

```
CREATE ROLE postgres SUPERUSER;
CREATE ROLE bar;

```



## services\.postgres\.listen_addresses



Listen address



*Type:*
string



*Default:*
` "" `



*Example:*
` "127.0.0.1" `



## services\.postgres\.port



The TCP port to accept connections\.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 5432 `



## services\.postgres\.settings



PostgreSQL configuration\. Refer to
[https://www\.postgresql\.org/docs/11/config-setting\.html\#CONFIG-SETTING-CONFIGURATION-FILE](https://www\.postgresql\.org/docs/11/config-setting\.html\#CONFIG-SETTING-CONFIGURATION-FILE)
for an overview of ` postgresql.conf `\.

String values will automatically be enclosed in single quotes\. Single quotes will be
escaped with two single quotes as described by the upstream documentation linked above\.



*Type:*
attribute set of (boolean or floating point number or signed integer or string)



*Default:*
` { } `



*Example:*

```
{
  log_connections = true;
  log_statement = "all";
  logging_collector = true
  log_disconnections = true
  log_destination = lib.mkForce "syslog";
}

```
