#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${PGHOST:-}" || -z "${PGUSER:-}" || -z "${PGPASSWORD:-}" || -z "${PGDATABASE:-}" ]]; then
  echo "Missing PG envs: PGHOST, PGUSER, PGPASSWORD, PGDATABASE" >&2
  exit 1
fi

echo "Applying DB init SQL to $PGHOST/$PGDATABASE as $PGUSER"
echo "Pre-dropping known triggers/functions to allow idempotent apply..."
psql -v ON_ERROR_STOP=1 -h "$PGHOST" -U "$PGUSER" -d "$PGDATABASE" -c "DROP TRIGGER IF EXISTS transaction_programs_trigger ON transactions;"
psql -v ON_ERROR_STOP=1 -h "$PGHOST" -U "$PGUSER" -d "$PGDATABASE" -c "DROP FUNCTION IF EXISTS update_transaction_programs();"
psql -v ON_ERROR_STOP=1 -h "$PGHOST" -U "$PGUSER" -d "$PGDATABASE" -c "DROP FUNCTION IF EXISTS normalize_program_id(text);"
psql -v ON_ERROR_STOP=1 -h "$PGHOST" -U "$PGUSER" -d "$PGDATABASE" -c "DROP FUNCTION IF EXISTS decode_base58(text);"
# Native balances trigger/function (idempotency)
psql -v ON_ERROR_STOP=0 -h "$PGHOST" -U "$PGUSER" -d "$PGDATABASE" -c "DROP TRIGGER IF EXISTS native_balances_trigger ON transactions;" || true
psql -v ON_ERROR_STOP=0 -h "$PGHOST" -U "$PGUSER" -d "$PGDATABASE" -c "DROP FUNCTION IF EXISTS populate_native_balances_from_tx();" || true

echo "Truncating data tables (if present) to ensure fresh sync..."
for t in \
  transaction_programs \
  programs \
  transactions \
  blocks \
  mempool_transactions \
  token_balances \
  token_accounts \
  token_mints \
  account_participation; do
  echo " - truncating $t (if exists)"
  # TRUNCATE does not support IF EXISTS; try and ignore missing-table errors
  psql -v ON_ERROR_STOP=0 -h "$PGHOST" -U "$PGUSER" -d "$PGDATABASE" -c "TRUNCATE TABLE \"$t\" CASCADE;" || true
done
shopt -s nullglob
for f in /db-init/*.sql; do
  echo "==> Running $(basename "$f")"
  psql -v ON_ERROR_STOP=1 -h "$PGHOST" -U "$PGUSER" -d "$PGDATABASE" -f "$f"
done
echo "All db-init SQL applied successfully."
