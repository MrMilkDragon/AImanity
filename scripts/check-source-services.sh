#!/usr/bin/env bash
set -euo pipefail

fixture=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/aimanity-source-ci.XXXXXX")
pg_bin=$(pg_config --bindir)
export AI_PHONE_TEST_PG_CTL="$pg_bin/pg_ctl"
export AI_PHONE_TEST_PGDATA="$fixture/postgres"
export AI_PHONE_TEST_PG_PORT=55500
brokers=()
cleanup() {
  for broker in "${brokers[@]}"; do
    kill "$broker" 2>/dev/null || true
    wait "$broker" || true
  done
  if "$AI_PHONE_TEST_PG_CTL" -D "$AI_PHONE_TEST_PGDATA" status >/dev/null 2>&1; then
    "$AI_PHONE_TEST_PG_CTL" -D "$AI_PHONE_TEST_PGDATA" -m immediate stop >/dev/null
  fi
  rm -rf -- "$fixture"
}
trap cleanup EXIT
printf '%s\n' isolated-fixture-password > "$fixture/password"
"$pg_bin/initdb" -D "$AI_PHONE_TEST_PGDATA" -U ai_phone --auth-local=trust --auth-host=scram-sha-256 \
  --pwfile="$fixture/password" --locale=C >/dev/null
rm "$fixture/password"
touch "$AI_PHONE_TEST_PGDATA/.aiphone-recovery-fixture"
printf "listen_addresses = '127.0.0.1'\nport = %s\nunix_socket_directories = '%s'\n" \
  "$AI_PHONE_TEST_PG_PORT" "$fixture" >> "$AI_PHONE_TEST_PGDATA/postgresql.conf"
"$AI_PHONE_TEST_PG_CTL" -D "$AI_PHONE_TEST_PGDATA" -l "$fixture/postgres.log" \
  -w start >/dev/null

export PGHOST=127.0.0.1 PGPORT="$AI_PHONE_TEST_PG_PORT" PGUSER=ai_phone PGPASSWORD=isolated-fixture-password
export AI_PHONE_TEST_DATABASE_URL="postgres://ai_phone:$PGPASSWORD@$PGHOST:$PGPORT/ai_phone"
export AI_PHONE_TEST_PERSISTENCE_DATABASE_URL="postgres://aiphone_test:isolated-persistence-fixture@$PGHOST:$PGPORT/ai_phone"
"$pg_bin/createdb" ai_phone
"$pg_bin/psql" -X -v ON_ERROR_STOP=1 -d ai_phone >/dev/null <<'SQL'
CREATE ROLE aiphone_test WITH LOGIN NOSUPERUSER CREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD 'isolated-persistence-fixture';
SQL

bin=$(cd "${CARGO_TARGET_DIR:-target}" && pwd)
resources=apps/native/gen/host
receipt=$(jq -er '."source.payload"' "$resources/integrity.json")
staging=$("$bin/debug/host-resources" stage "$resources/source.payload" "$fixture/staging" "$receipt")
staged=${staging#* }
"$bin/debug/release-tools" native-services verify "$staged"

database="$fixture/database"
mkdir "$database"
(
  umask 077
  printf 'POSTGRES_DB=ai_phone\nPOSTGRES_USER=ai_phone\nPOSTGRES_PASSWORD=%s\n' "$PGPASSWORD" > "$database/.env"
)
program="$staged/runtime/services/bin/assistant"
probe() {
  env -i HOME="$database" PATH=/usr/bin:/bin AI_PHONE_PROJECT_ROOT="$database" AI_PHONE_PG_PORT="$PGPORT" "$program" "$@"
}
probe --prepare-database
probe --migrate
probe --database-check | jq -e '.runtime == "ai_phone_runtime" and .privileged == false and .ddl == false'
runtime_db="$database/data/database/credentials.json"
export PGUSER="$(jq -er '.runtime.username' "$runtime_db")"
export PGPASSWORD="$(jq -er '.runtime.password' "$runtime_db")"
migrations=$("$pg_bin/psql" -X -tA -d ai_phone -c 'SELECT count(*) FROM _sqlx_migrations WHERE success;')
test "$migrations" -gt 0
if LC_ALL=C "$pg_bin/psql" -X -v ON_ERROR_STOP=1 -d ai_phone -c 'CREATE TABLE native_probe_forbidden(id int);' > "$fixture/ddl" 2>&1; then
  exit 1
fi
grep -q 'permission denied' "$fixture/ddl"
unset PGHOST PGPORT PGUSER PGPASSWORD
printf 'Packaged PostgreSQL runtime applied %d migrations and rejected DDL.\n' "$migrations"

cargo build --locked -p ai-phone-assistant --bin ai-phone-assistant
export AI_PHONE_TEST_ASSISTANT_BINARY="$bin/debug/ai-phone-assistant"
if [[ ${1:-} == --with-event-brokers ]]; then
  export AI_PHONE_TEST_REDIS_PORT=56383 AI_PHONE_TEST_KAFKA_BROKER=127.0.0.1:59092
  redis-server --bind 127.0.0.1 --port "$AI_PHONE_TEST_REDIS_PORT" --save '' \
    --appendonly no --dir "$fixture" > "$fixture/redis.log" 2>&1 &
  brokers+=("$!")

  kafka="$fixture/kafka"
  mkdir "$kafka"
  curl -fsSL https://archive.apache.org/dist/kafka/4.4.0/kafka_2.13-4.4.0.tgz \
    | tar -xz --strip-components=1 -C "$kafka"
  cat > "$fixture/kafka.properties" <<EOF
node.id=1
process.roles=broker,controller
controller.quorum.bootstrap.servers=127.0.0.1:59093
listeners=PLAINTEXT://$AI_PHONE_TEST_KAFKA_BROKER,CONTROLLER://127.0.0.1:59093
advertised.listeners=PLAINTEXT://$AI_PHONE_TEST_KAFKA_BROKER
controller.listener.names=CONTROLLER
listener.security.protocol.map=CONTROLLER:PLAINTEXT,PLAINTEXT:PLAINTEXT
offsets.topic.replication.factor=1
auto.create.topics.enable=false
log.dirs=$fixture/kafka-data
EOF
  export KAFKA_HEAP_OPTS='-Xms128m -Xmx256m'
  "$kafka/bin/kafka-storage.sh" format --standalone --cluster-id 4L6g3nShT-eMCtK--X86sw -c "$fixture/kafka.properties"
  "$kafka/bin/kafka-server-start.sh" "$fixture/kafka.properties" > "$fixture/kafka.log" 2>&1 &
  brokers+=("$!")
  if ! timeout 45 "$kafka/bin/kafka-topics.sh" --bootstrap-server "$AI_PHONE_TEST_KAFKA_BROKER" --list; then
    cat "$fixture/kafka.log"
    exit 1
  fi
fi
exclude=(--skip model_adapter::tests::live_model_capabilities --skip worker::process_recovery_tests::worker_child)
if [[ -z ${AI_PHONE_TEST_REDIS_PORT:-} ]]; then
  exclude+=(--skip redis_events::tests::committed_wake_reaches_channel)
fi
if [[ -z ${AI_PHONE_TEST_KAFKA_BROKER:-} ]]; then
  exclude+=(--skip device_exchange::kafka::tests::kafka_handoff_replay)
fi
cargo test --locked -p ai-phone-assistant --bin ai-phone-assistant -- --ignored --test-threads=1 "${exclude[@]}"
