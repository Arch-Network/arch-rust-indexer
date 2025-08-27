// Centralized Arch and legacy (Solana) program IDs in base58

// Arch canonical program IDs
pub const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";
pub const VOTE_PROGRAM: &str = "VoteProgram111111111111111111111";
pub const STAKE_PROGRAM: &str = "StakeProgram11111111111111111111";
pub const BPF_LOADER: &str = "BpfLoader11111111111111111111111";
pub const NATIVE_LOADER: &str = "NativeLoader11111111111111111111";
pub const COMPUTE_BUDGET: &str = "ComputeBudget111111111111111111111111111111";

// Arch token programs (fixed canonical IDs)
pub const APL_TOKEN_PROGRAM: &str = "AplToken111111111111111111111111";
pub const APL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM: &str = "AssociatedTokenAccount1111111111";

// Legacy Solana program IDs we may still see in historical/interop data
pub const SOL_LOADER: &str = "Loader1111111111111111111111111111111";
pub const SOL_COMPUTE_BUDGET: &str = "ComputeBudget111111111111111111111111111111";
pub const SOL_MEMO: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
pub const SOL_SPL_TOKEN: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const SOL_ASSOCIATED_TOKEN_ACCOUNT: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLkek";

// Pre-computed base58 representations of our string constants
// These are computed using: bs58::encode(string_const.as_bytes()).into_string()
// The from_slice function pads the ASCII bytes to 32 bytes, then converts to base58
pub const VOTE_PROGRAM_BASE58: &str = "6pQdihBHh2RjkvynAKrVRXiJwWqQ3etv4FgsnKMz1yZ2";
pub const STAKE_PROGRAM_BASE58: &str = "6cmmTNBUrk1xPATwae9QiZWQxrAqPx2G28xEG9vinJ3a";
pub const BPF_LOADER_BASE58: &str = "5UMKG6S4Dn9JLbJvCsE3qqPd2KcbpRypdNHG2KLP7Bex";
pub const NATIVE_LOADER_BASE58: &str = "6GxzQyi6PJRtuTZMeCYCec5bUf2W26uVhKTzch4nE3ax";
pub const COMPUTE_BUDGET_BASE58: &str = "5y7q1eo4EQ6UK8Z7xwQn4ZBVxzVQjKP9rKdwPpnomGfYwGnoXMGDwzKyvSL";
pub const APL_TOKEN_PROGRAM_BASE58: &str = "5QSvph6op2FQj23To5H2LpD5unF1KXmVz29gFMoJTEoJ";
pub const APL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_BASE58: &str = "5QVc8gaXMdjnfS8JS1K8NbQQVPhVHfVPY2asS8b1xY8g";

/// Maps base58 program IDs to their human-readable names
pub fn get_program_name(base58_id: &str) -> Option<&'static str> {
    match base58_id {
        // Direct base58 matches
        SYSTEM_PROGRAM => Some("System Program"),
        SOL_LOADER => Some("BPF Loader"),
        SOL_COMPUTE_BUDGET => Some("Compute Budget Program"),
        SOL_MEMO => Some("Memo Program"),
        SOL_SPL_TOKEN => Some("SPL Token Program"),
        SOL_ASSOCIATED_TOKEN_ACCOUNT => Some("Associated Token Account Program"),
        
        // Arch programs - check each one individually to avoid unreachable pattern warnings
        VOTE_PROGRAM_BASE58 => Some("Vote Program"),
        STAKE_PROGRAM_BASE58 => Some("Stake Program"),
        BPF_LOADER_BASE58 => Some("BPF Loader"),
        NATIVE_LOADER_BASE58 => Some("Native Loader"),
        COMPUTE_BUDGET_BASE58 => Some("Compute Budget Program"),
        APL_TOKEN_PROGRAM_BASE58 => Some("APL Token Program"),
        APL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_BASE58 => Some("APL Associated Token Account Program"),
        
        _ => None,
    }
}

/// Maps a base58 program ID to its display name, falling back to the base58 if unknown
pub fn get_program_display_name(base58_id: &str) -> String {
    get_program_name(base58_id)
        .map(|name| name.to_string())
        .unwrap_or_else(|| base58_id.to_string())
}
