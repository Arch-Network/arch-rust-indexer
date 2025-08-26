#!/bin/bash

# Script to apply the token balances migration
# This script should be run from the rust/ directory

set -e

echo "Applying token balances migration..."

# Check if config.yml exists
if [ ! -f "config.yml" ]; then
    echo "Error: config.yml not found. Please run this script from the rust/ directory."
    exit 1
fi

# Extract database connection details from config.yml
DB_HOST=$(grep "host:" config.yml | awk '{print $2}' | tr -d '"')
DB_PORT=$(grep "port:" config.yml | awk '{print $2}' | tr -d '"')
DB_NAME=$(grep "database:" config.yml | awk '{print $2}' | tr -d '"')
DB_USER=$(grep "username:" config.yml | awk '{print $2}' | tr -d '"')
DB_PASS=$(grep "password:" config.yml | awk '{print $2}' | tr -d '"')

if [ -z "$DB_HOST" ] || [ -z "$DB_PORT" ] || [ -z "$DB_NAME" ] || [ -z "$DB_USER" ]; then
    echo "Error: Could not extract database connection details from config.yml"
    exit 1
fi

echo "Database: $DB_NAME on $DB_HOST:$DB_PORT"
echo "User: $DB_USER"

# Apply the migration
echo "Applying migration: 20250103000000_add_token_balances.sql"
PGPASSWORD="$DB_PASS" psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" -f migrations/20250103000000_add_token_balances.sql

echo "Migration applied successfully!"
echo ""
echo "New tables created:"
echo "- token_balances: Stores token balance information for accounts"
echo "- token_mints: Stores token mint metadata"
echo ""
echo "New view created:"
echo "- account_token_balances: Combined view for easy token balance queries"
echo ""
echo "New trigger created:"
echo "- token_balances_trigger: Automatically updates token balances when transactions are inserted"
