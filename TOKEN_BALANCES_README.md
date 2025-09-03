# Token Balances Feature

This document describes the implementation of the token balances feature for the Arch Explorer.

## Overview

The token balances feature allows users to view token holdings for any account address. It tracks:
- Token mint addresses
- Account balances
- Token metadata (decimals, supply, etc.)
- Program information
- Last update timestamps

## Database Schema

### Tables

#### `token_balances`
Stores token balance information for accounts:
- `id`: Primary key
- `account_address`: The account address (hex format)
- `mint_address`: The token mint address (hex format)
- `balance`: Current token balance
- `decimals`: Token decimal places
- `owner_address`: Account owner address
- `program_id`: Token program ID
- `last_updated`: Last update timestamp
- `created_at`: Creation timestamp

#### `token_mints`
Stores token mint metadata:
- `mint_address`: Primary key, token mint address
- `program_id`: Token program ID
- `decimals`: Token decimal places
- `supply`: Total token supply
- `is_frozen`: Whether the mint is frozen
- `mint_authority`: Mint authority address
- `freeze_authority`: Freeze authority address
- `first_seen_at`: First seen timestamp
- `last_seen_at`: Last seen timestamp

### Views

#### `account_token_balances`
Combined view for easy token balance queries, joining `token_balances` with `token_mints`.

### Triggers

#### `token_balances_trigger`
Automatically updates token balances when new transactions are inserted into the `transactions` table.

## API Endpoints

### GET `/api/accounts/:address/token-balances`

Returns token balances for a specific account address.

**Query Parameters:**
- `page`: Page number (default: 1)
- `limit`: Items per page (default: 25, max: 200)

**Response:**
```json
{
  "page": 1,
  "limit": 25,
  "balances": [
    {
      "mint_address": "base58_encoded_mint_address",
      "mint_address_hex": "hex_encoded_mint_address",
      "balance": "1000000",
      "decimals": 6,
      "owner_address": "base58_encoded_owner",
      "program_id": "base58_encoded_program",
      "program_name": "APL Token",
      "supply": "1000000000",
      "is_frozen": false,
      "last_updated": "2025-01-03T12:00:00Z"
    }
  ],
  "total": 5
}
```

## Frontend Implementation

The token balances are displayed in a new tab on the account page with:
- Pagination controls
- Table showing mint address, balance, program, supply, and last updated
- Support for both raw and formatted balance display
- Program name resolution

## Setup Instructions

### 1. Apply Database Migration

Run the migration script from the `rust/` directory:

```bash
./scripts/apply_token_balances_migration.sh
```

This will:
- Create the necessary tables
- Set up indexes for performance
- Create the trigger function
- Create the view for easy querying

### 2. Restart API Server

After applying the migration, restart the API server to ensure the new endpoint is available.

### 3. Test the Feature

Navigate to any account page and click on the "Token Balances" tab to see the feature in action.

## Current Limitations

1. **Balance Calculation**: The current implementation is a placeholder. Real balance calculation requires parsing actual instruction data from transactions.

2. **Token Program Support**: Currently supports:
   - APL Token Program (`7ZMyUmgbNckx7G5BCrdmX2XUasjDAk5uhcMpDbUDxHQ3`)
   - SPL Token Program (`TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA`)

3. **Data Population**: Token balances will only be populated for new transactions after the migration is applied.

## Future Enhancements

1. **Real Balance Calculation**: Implement proper parsing of token instruction data to calculate actual balances.

2. **Historical Data**: Add support for historical balance tracking over time.

3. **Token Metadata**: Integrate with token metadata services for additional information (symbol, name, logo, etc.).

4. **Balance Changes**: Track and display balance change history.

5. **Token Transfers**: Show incoming/outgoing token transfers for accounts.

## Technical Notes

- The feature gracefully handles cases where the `token_balances` table doesn't exist yet
- Uses the existing `normalize_program_id` function for address normalization
- Integrates with the existing program name resolution system
- Follows the same pagination pattern as other account endpoints
- Uses the existing error handling and response formatting patterns
