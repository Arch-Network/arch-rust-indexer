use arch_program::pubkey::Pubkey;

fn main() {
    let programs = vec![
        ("VOTE_PROGRAM", "VoteProgram111111111111111111111"),
        ("STAKE_PROGRAM", "StakeProgram11111111111111111111"),
        ("BPF_LOADER", "BpfLoader11111111111111111111111"),
        ("NATIVE_LOADER", "NativeLoader11111111111111111111"),
        ("COMPUTE_BUDGET", "ComputeBudget111111111111111111111111111111"),
        ("APL_TOKEN_PROGRAM", "AplToken111111111111111111111111"),
        ("APL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM", "AssociatedTokenAccount1111111111"),
    ];

    println!("// Pre-computed base58 representations:");
    for (name, string_const) in programs {
        let pubkey = Pubkey::from_slice(string_const.as_bytes());
        let base58 = pubkey.to_string();
        println!("pub const {}_BASE58: &str = \"{}\";", name, base58);
    }
} 