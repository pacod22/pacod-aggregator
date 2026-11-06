#!/bin/sh
# pacod-aggregator's role and database on the shared PostgreSQL (pacod-postgres): a role "pacod_agg"
# that is not a superuser and owns the database "pacod_agg". The server creates its tables there
# (migrations run at start); none of them needs a superuser.
#
# pacod-db-init runs this at every start, before the server, as a superuser through libpq's PG*
# variables (PGHOST, PGUSER, PGPASSWORD). Safe to run again: it creates what is missing and sets the
# role's password to AGG_DB_PASSWORD. The password is read with psql's \getenv, so it is on no
# command line, and nothing secret is printed. The same file serves the aggregator's own stack
# (docker-compose.standalone.yml) and the full stack (pacod-demo).

set -eu

: "${PGHOST:?the PostgreSQL host, with PGUSER and PGPASSWORD of a superuser}"
: "${AGG_DB_PASSWORD:?the password for the role pacod_agg}"

psql -v ON_ERROR_STOP=1 -q -d postgres <<'SQL'
\getenv pw AGG_DB_PASSWORD
SELECT format('CREATE ROLE pacod_agg LOGIN PASSWORD %L', :'pw')
 WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'pacod_agg') \gexec
SELECT format('ALTER ROLE pacod_agg LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD %L', :'pw') \gexec
SELECT 'CREATE DATABASE pacod_agg OWNER pacod_agg'
 WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'pacod_agg') \gexec
REVOKE ALL ON DATABASE pacod_agg FROM PUBLIC;
GRANT CONNECT, CREATE, TEMPORARY ON DATABASE pacod_agg TO pacod_agg;
SQL

echo "agg-db: the role and the database pacod_agg are in place"
