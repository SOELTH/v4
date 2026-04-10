use std::str::FromStr;

use clap::Args;
use colored::Colorize;
use solana_program::pubkey;
use solana_sdk::pubkey::Pubkey;
use squads_multisig::anchor_lang::AccountDeserialize;
use squads_multisig::pda::get_transaction_pda;
use squads_multisig::solana_rpc_client::nonblocking::rpc_client::RpcClient;
use squads_multisig::squads_multisig_program::state::{
    Batch, ConfigAction, ConfigTransaction, VaultTransaction,
};

// Well-known program IDs
const TOKEN_2022_ID: Pubkey = pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
const COMPUTE_BUDGET_ID: Pubkey = pubkey!("ComputeBudget111111111111111111111111111111");
const BPF_UPGRADEABLE_LOADER_ID: Pubkey = pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");
const SQUADS_V4_ID: Pubkey = pubkey!("SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf");
const JUPITER_V6_ID: Pubkey = pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");

// Well-known mints
const USDC_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const USDT_MINT: Pubkey = pubkey!("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB");
const WSOL_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
const MSOL_MINT: Pubkey = pubkey!("mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So");
const STSOL_MINT: Pubkey = pubkey!("7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj");
const JITOSOL_MINT: Pubkey = pubkey!("J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn");
const BONK_MINT: Pubkey = pubkey!("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263");
const JUP_MINT: Pubkey = pubkey!("JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN");

/// Fetch and display the contents of a multisig transaction (vault, config, or batch).
#[derive(Args)]
pub struct DisplayTransaction {
    /// RPC URL
    #[arg(long)]
    rpc_url: Option<String>,

    /// Multisig Program ID
    #[arg(long)]
    program_id: Option<String>,

    /// The multisig pubkey
    #[arg(long)]
    multisig_pubkey: String,

    /// The transaction index to display
    #[arg(long)]
    transaction_index: u64,
}

impl DisplayTransaction {
    pub async fn execute(self) -> eyre::Result<()> {
        let program_id = Pubkey::from_str(
            &self
                .program_id
                .unwrap_or_else(|| SQUADS_V4_ID.to_string()),
        )
        .expect("Invalid program ID");

        let rpc_url = self
            .rpc_url
            .unwrap_or_else(|| "https://api.mainnet-beta.solana.com".to_string());
        let rpc_client = RpcClient::new(rpc_url.clone());

        let multisig =
            Pubkey::from_str(&self.multisig_pubkey).expect("Invalid multisig address");
        let tx_pda =
            get_transaction_pda(&multisig, self.transaction_index, Some(&program_id));

        println!();
        println!("{}", "Fetching transaction details...".yellow());
        println!();
        println!("RPC Cluster URL:   {}", rpc_url);
        println!("Program ID:        {}", program_id);
        println!("Multisig Key:      {}", self.multisig_pubkey);
        println!("Transaction PDA:   {}", tx_pda.0);
        println!();

        let account = rpc_client
            .get_account(&tx_pda.0)
            .await
            .map_err(|e| eyre::eyre!("Failed to fetch transaction account: {}", e))?;

        // Try VaultTransaction first, then ConfigTransaction, then Batch.
        // Anchor discriminators are unique per type so exactly one will match.
        if let Ok(vault_tx) = VaultTransaction::try_deserialize(&mut account.data.as_slice()) {
            display_vault_transaction(&rpc_client, &vault_tx).await?;
        } else if let Ok(config_tx) =
            ConfigTransaction::try_deserialize(&mut account.data.as_slice())
        {
            display_config_transaction(&config_tx);
        } else if let Ok(batch) = Batch::try_deserialize(&mut account.data.as_slice()) {
            println!("Type:             {}", "Batch Transaction".cyan());
            println!("Creator:          {}", batch.creator);
            println!("Index:            {}", batch.index);
            println!("Vault Index:      {}", batch.vault_index);
            println!("Batch Size:       {}", batch.size);
            println!("Executed So Far:  {}", batch.executed_transaction_index);
        } else {
            return Err(eyre::eyre!(
                "Failed to deserialize transaction account as VaultTransaction, ConfigTransaction, or Batch"
            ));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Vault transaction display
// ---------------------------------------------------------------------------

async fn display_vault_transaction(
    rpc_client: &RpcClient,
    vault_tx: &VaultTransaction,
) -> eyre::Result<()> {
    println!("Type:             {}", "Vault Transaction".cyan());
    println!("Creator:          {}", vault_tx.creator);
    println!("Index:            {}", vault_tx.index);
    println!("Vault Index:      {}", vault_tx.vault_index);
    if !vault_tx.ephemeral_signer_bumps.is_empty() {
        println!(
            "Ephemeral Signers: {}",
            vault_tx.ephemeral_signer_bumps.len()
        );
    }
    println!();

    let msg = &vault_tx.message;

    // Build the full resolved account key list: static keys, then ALT writable, then ALT readonly.
    // This ordering matches how instruction indexes reference accounts.
    let all_keys = resolve_account_keys(rpc_client, msg).await?;

    // Print account keys
    println!("{}", "Account Keys:".yellow());
    for (i, rk) in all_keys.iter().enumerate() {
        print_resolved_key(i, rk);
    }

    // Print instructions
    println!();
    println!("{}", "Instructions:".yellow());

    for (i, ix) in msg.instructions.iter().enumerate() {
        println!("{}", SEPARATOR.bright_black());

        let program_key = resolve_key(&all_keys, ix.program_id_index);
        let program_name = identify_program(&program_key).unwrap_or("Unknown Program");

        println!(
            "  {} {}",
            format!("Instruction {}:", i).cyan(),
            program_name.yellow(),
        );

        // Resolve account pubkeys for decoding
        let ix_accounts: Vec<Pubkey> = ix
            .account_indexes
            .iter()
            .map(|&idx| resolve_key(&all_keys, idx))
            .collect();

        // Decode and print human-readable summary if possible
        let decoded = decode_instruction(&program_key, &ix.data, &ix_accounts);
        if let Some(ref desc) = decoded {
            println!("  {}", desc);
        }

        // Print raw accounts
        println!("  {}", "Accounts:".bright_black());
        for &acc_idx in &ix.account_indexes {
            if let Some(rk) = all_keys.get(acc_idx as usize) {
                let tag_str = format_tags_short(rk);
                let name_str = format_program_label(&rk.pubkey);
                println!(
                    "    [{}] {}{}{}",
                    format!("{}", acc_idx).bright_black(),
                    rk.pubkey,
                    tag_str,
                    name_str,
                );
            } else {
                println!(
                    "    [{}] {}",
                    format!("{}", acc_idx).bright_black(),
                    "<out of range>".red(),
                );
            }
        }

        // Print raw hex data for unrecognized instructions
        if decoded.is_none() && !ix.data.is_empty() {
            let max_preview = 64;
            let hex = to_hex(&ix.data[..ix.data.len().min(max_preview)]);
            let suffix = if ix.data.len() > max_preview {
                "..."
            } else {
                ""
            };
            println!(
                "  {} {} bytes: {}{}",
                "Data:".bright_black(),
                ix.data.len(),
                hex,
                suffix,
            );
        }
    }

    println!("{}", SEPARATOR.bright_black());
    Ok(())
}

/// Resolve all account keys for a vault transaction message, including ALT lookups.
async fn resolve_account_keys(
    rpc_client: &RpcClient,
    msg: &squads_multisig::squads_multisig_program::state::VaultTransactionMessage,
) -> eyre::Result<Vec<ResolvedKey>> {
    let mut all_keys = Vec::new();

    // Static keys
    for (i, key) in msg.account_keys.iter().enumerate() {
        all_keys.push(ResolvedKey {
            pubkey: *key,
            writable: msg.is_static_writable_index(i),
            signer: msg.is_signer_index(i),
            source: KeySource::Static,
        });
    }

    if msg.address_table_lookups.is_empty() {
        return Ok(all_keys);
    }

    // Batch-fetch all ALTs in a single RPC call
    let alt_pubkeys: Vec<Pubkey> = msg
        .address_table_lookups
        .iter()
        .map(|l| l.account_key)
        .collect();

    let alt_accounts = rpc_client
        .get_multiple_accounts(&alt_pubkeys)
        .await
        .map_err(|e| eyre::eyre!("Failed to fetch address lookup tables: {}", e))?;

    for (lookup, alt_account) in msg.address_table_lookups.iter().zip(alt_accounts.iter()) {
        let Some(alt_account) = alt_account else {
            // ALT not found — push placeholders so indexes stay correct
            push_unresolved_keys(&mut all_keys, lookup);
            continue;
        };

        let table = solana_address_lookup_table_interface::state::AddressLookupTable::deserialize(
            &alt_account.data,
        )
        .map_err(|e| {
            eyre::eyre!("Failed to deserialize ALT {}: {}", lookup.account_key, e)
        })?;

        // Writable keys first, then readonly — matching the on-chain index ordering
        for &idx in &lookup.writable_indexes {
            all_keys.push(ResolvedKey {
                pubkey: table.addresses.get(idx as usize).copied().unwrap_or_default(),
                writable: true,
                signer: false,
                source: KeySource::Lookup(lookup.account_key, idx),
            });
        }
        for &idx in &lookup.readonly_indexes {
            all_keys.push(ResolvedKey {
                pubkey: table.addresses.get(idx as usize).copied().unwrap_or_default(),
                writable: false,
                signer: false,
                source: KeySource::Lookup(lookup.account_key, idx),
            });
        }
    }

    Ok(all_keys)
}

fn push_unresolved_keys(
    all_keys: &mut Vec<ResolvedKey>,
    lookup: &squads_multisig::squads_multisig_program::state::MultisigMessageAddressTableLookup,
) {
    let source = KeySource::LookupUnresolved(lookup.account_key);
    for _ in &lookup.writable_indexes {
        all_keys.push(ResolvedKey {
            pubkey: Pubkey::default(),
            writable: true,
            signer: false,
            source: source.clone(),
        });
    }
    for _ in &lookup.readonly_indexes {
        all_keys.push(ResolvedKey {
            pubkey: Pubkey::default(),
            writable: false,
            signer: false,
            source: source.clone(),
        });
    }
}

// ---------------------------------------------------------------------------
// Config transaction display
// ---------------------------------------------------------------------------

fn display_config_transaction(config_tx: &ConfigTransaction) {
    println!("Type:             {}", "Config Transaction".cyan());
    println!("Creator:          {}", config_tx.creator);
    println!("Index:            {}", config_tx.index);
    println!();
    println!(
        "{}",
        format!("Actions ({}):", config_tx.actions.len()).yellow()
    );

    for (i, action) in config_tx.actions.iter().enumerate() {
        println!("{}", SEPARATOR.bright_black());
        let label = format!("Action {}:", i).cyan();
        match action {
            ConfigAction::AddMember { new_member } => {
                println!("  {} {}", label, "Add Member".yellow());
                println!("    Key:         {}", new_member.key);
                println!(
                    "    Permissions: {}",
                    format_permissions(new_member.permissions.mask)
                );
            }
            ConfigAction::RemoveMember { old_member } => {
                println!("  {} {}", label, "Remove Member".yellow());
                println!("    Key: {}", old_member);
            }
            ConfigAction::ChangeThreshold { new_threshold } => {
                println!("  {} {}", label, "Change Threshold".yellow());
                println!("    New Threshold: {}", new_threshold);
            }
            ConfigAction::SetTimeLock { new_time_lock } => {
                println!("  {} {}", label, "Set Time Lock".yellow());
                println!("    New Time Lock: {} seconds", new_time_lock);
            }
            ConfigAction::AddSpendingLimit {
                create_key,
                vault_index,
                mint,
                amount,
                period,
                members,
                destinations,
            } => {
                println!("  {} {}", label, "Add Spending Limit".yellow());
                println!("    Create Key:  {}", create_key);
                println!("    Vault Index: {}", vault_index);
                println!("    Mint:        {}{}", mint, format_program_label(mint));
                println!("    Amount:      {}", amount);
                println!("    Period:      {}", format_period(period));
                println!("    Members:");
                for m in members {
                    println!("      - {}", m);
                }
                if destinations.is_empty() {
                    println!("    Destinations: any");
                } else {
                    println!("    Destinations:");
                    for d in destinations {
                        println!("      - {}", d);
                    }
                }
            }
            ConfigAction::RemoveSpendingLimit { spending_limit } => {
                println!("  {} {}", label, "Remove Spending Limit".yellow());
                println!("    Spending Limit: {}", spending_limit);
            }
            ConfigAction::SetRentCollector { new_rent_collector } => {
                println!("  {} {}", label, "Set Rent Collector".yellow());
                match new_rent_collector {
                    Some(pk) => println!("    Rent Collector: {}", pk),
                    None => println!("    Rent Collector: None (disabled)"),
                }
            }
            _ => {
                println!("  {} {}", label, "Unknown Config Action".yellow());
            }
        }
    }
    println!("{}", SEPARATOR.bright_black());
}

fn format_permissions(mask: u8) -> String {
    let mut perms = Vec::new();
    if mask & 1 != 0 {
        perms.push("Initiate");
    }
    if mask & 2 != 0 {
        perms.push("Vote");
    }
    if mask & 4 != 0 {
        perms.push("Execute");
    }
    if perms.is_empty() {
        format!("None ({})", mask)
    } else {
        format!("{} ({})", perms.join(", "), mask)
    }
}

fn format_period(
    period: &squads_multisig::squads_multisig_program::state::Period,
) -> &'static str {
    use squads_multisig::squads_multisig_program::state::Period;
    match period {
        Period::OneTime => "One-Time",
        Period::Day => "Daily",
        Period::Week => "Weekly",
        Period::Month => "Monthly",
    }
}

// ---------------------------------------------------------------------------
// Instruction decoding
// ---------------------------------------------------------------------------

fn decode_instruction(program_id: &Pubkey, data: &[u8], accounts: &[Pubkey]) -> Option<String> {
    if *program_id == solana_sdk::system_program::id() {
        decode_system(data, accounts)
    } else if *program_id == spl_token::id() || *program_id == TOKEN_2022_ID {
        decode_token(data, accounts)
    } else if *program_id == spl_associated_token_account::id() {
        decode_ata(data, accounts)
    } else if *program_id == COMPUTE_BUDGET_ID {
        decode_compute_budget(data)
    } else {
        None
    }
}

fn decode_system(data: &[u8], accounts: &[Pubkey]) -> Option<String> {
    if data.len() < 4 {
        return None;
    }
    let disc = u32::from_le_bytes(data[0..4].try_into().ok()?);
    match disc {
        2 if data.len() >= 12 && accounts.len() >= 2 => {
            let lamports = u64::from_le_bytes(data[4..12].try_into().ok()?);
            let sol = lamports as f64 / 1_000_000_000.0;
            Some(format!(
                "Transfer {} SOL ({} lamports)\n    From: {}\n    To:   {}",
                sol, lamports, accounts[0], accounts[1],
            ))
        }
        _ => None,
    }
}

fn decode_token(data: &[u8], accounts: &[Pubkey]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    match data[0] {
        // Transfer
        3 if data.len() >= 9 && accounts.len() >= 3 => {
            let amount = u64::from_le_bytes(data[1..9].try_into().ok()?);
            Some(format!(
                "Transfer {} tokens\n    From:      {}\n    To:        {}\n    Authority: {}",
                amount, accounts[0], accounts[1], accounts[2],
            ))
        }
        // TransferChecked
        12 if data.len() >= 10 && accounts.len() >= 4 => {
            let amount = u64::from_le_bytes(data[1..9].try_into().ok()?);
            let decimals = data[9];
            let human = format_token_amount(amount, decimals);
            let mint_name = identify_mint(&accounts[1]).unwrap_or("tokens");
            let mint_label = match identify_mint(&accounts[1]) {
                Some(name) => format!("{} ({})", accounts[1], name),
                None => accounts[1].to_string(),
            };
            Some(format!(
                "TransferChecked {} {} (raw: {}, decimals: {})\n    From:      {}\n    Mint:      {}\n    To:        {}\n    Authority: {}",
                human, mint_name, amount, decimals,
                accounts[0], mint_label, accounts[2], accounts[3],
            ))
        }
        _ => None,
    }
}

fn decode_ata(data: &[u8], accounts: &[Pubkey]) -> Option<String> {
    if accounts.len() < 4 {
        return None;
    }
    let variant = if data.is_empty() { 0u8 } else { data[0] };
    let kind = match variant {
        0 => "Create ATA",
        1 => "Create ATA (idempotent)",
        _ => return None,
    };
    let mint_label = match identify_mint(&accounts[3]) {
        Some(name) => format!("{} ({})", accounts[3], name),
        None => accounts[3].to_string(),
    };
    Some(format!(
        "{}\n    Funder: {}\n    ATA:    {}\n    Owner:  {}\n    Mint:   {}",
        kind, accounts[0], accounts[1], accounts[2], mint_label,
    ))
}

fn decode_compute_budget(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    match data[0] {
        2 if data.len() >= 5 => {
            let limit = u32::from_le_bytes(data[1..5].try_into().ok()?);
            Some(format!("Set compute unit limit: {}", limit))
        }
        3 if data.len() >= 9 => {
            let price = u64::from_le_bytes(data[1..9].try_into().ok()?);
            Some(format!("Set compute unit price: {} micro-lamports", price))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Well-known program & mint identification
// ---------------------------------------------------------------------------

fn identify_program(pubkey: &Pubkey) -> Option<&'static str> {
    match *pubkey {
        p if p == solana_sdk::system_program::id() => Some("System Program"),
        p if p == spl_token::id() => Some("SPL Token"),
        p if p == spl_associated_token_account::id() => Some("Associated Token Account"),
        p if p == solana_sdk::sysvar::rent::id() => Some("Rent Sysvar"),
        TOKEN_2022_ID => Some("SPL Token-2022"),
        COMPUTE_BUDGET_ID => Some("Compute Budget"),
        BPF_UPGRADEABLE_LOADER_ID => Some("BPF Upgradeable Loader"),
        SQUADS_V4_ID => Some("Squads v4"),
        JUPITER_V6_ID => Some("Jupiter v6"),
        _ => None,
    }
}

fn identify_mint(pubkey: &Pubkey) -> Option<&'static str> {
    match *pubkey {
        USDC_MINT => Some("USDC"),
        USDT_MINT => Some("USDT"),
        WSOL_MINT => Some("SOL"),
        MSOL_MINT => Some("mSOL"),
        STSOL_MINT => Some("stSOL"),
        JITOSOL_MINT => Some("JitoSOL"),
        BONK_MINT => Some("BONK"),
        JUP_MINT => Some("JUP"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

const SEPARATOR: &str = "────────────────────────────────────────────────────────────";

#[derive(Clone)]
struct ResolvedKey {
    pubkey: Pubkey,
    writable: bool,
    signer: bool,
    source: KeySource,
}

#[derive(Clone)]
enum KeySource {
    Static,
    Lookup(Pubkey, u8),
    LookupUnresolved(Pubkey),
}

fn resolve_key(keys: &[ResolvedKey], index: u8) -> Pubkey {
    keys.get(index as usize)
        .map(|rk| rk.pubkey)
        .unwrap_or_default()
}

fn print_resolved_key(i: usize, rk: &ResolvedKey) {
    let mut tags = Vec::new();
    if rk.signer {
        tags.push("signer".green().to_string());
    }
    if rk.writable {
        tags.push("writable".red().to_string());
    }
    let tag_str = if tags.is_empty() {
        String::new()
    } else {
        format!("  [{}]", tags.join(", "))
    };

    let source_str = match &rk.source {
        KeySource::Static => String::new(),
        KeySource::Lookup(alt, idx) => format!("  (ALT {}[{}])", truncate_pubkey(alt), idx)
            .bright_black()
            .to_string(),
        KeySource::LookupUnresolved(alt) => {
            format!("  (ALT {} unresolved)", truncate_pubkey(alt))
                .bright_black()
                .to_string()
        }
    };

    println!(
        "  [{}] {}{}{}{}",
        format!("{}", i).bright_black(),
        rk.pubkey,
        tag_str,
        format_program_label(&rk.pubkey),
        source_str,
    );
}

fn format_tags_short(rk: &ResolvedKey) -> String {
    let mut tags = Vec::new();
    if rk.writable {
        tags.push("w".red().to_string());
    }
    if rk.signer {
        tags.push("s".green().to_string());
    }
    if tags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", tags.join(","))
    }
}

fn format_program_label(pubkey: &Pubkey) -> String {
    match identify_program(pubkey) {
        Some(n) => format!("  ({})", n).bright_black().to_string(),
        None => String::new(),
    }
}

/// Format a token amount with decimal places using integer arithmetic to avoid
/// floating-point precision loss for large amounts.
fn format_token_amount(amount: u64, decimals: u8) -> String {
    if decimals == 0 {
        return amount.to_string();
    }
    let divisor = 10u64.pow(decimals as u32);
    let whole = amount / divisor;
    let frac = amount % divisor;
    // Pad fractional part to the correct width, then trim trailing zeros
    let frac_str = format!("{:0>width$}", frac, width = decimals as usize);
    let frac_trimmed = frac_str.trim_end_matches('0');
    if frac_trimmed.is_empty() {
        whole.to_string()
    } else {
        format!("{}.{}", whole, frac_trimmed)
    }
}

fn truncate_pubkey(pubkey: &Pubkey) -> String {
    let s = pubkey.to_string();
    if s.len() > 11 {
        format!("{}..{}", &s[..4], &s[s.len() - 4..])
    } else {
        s
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
