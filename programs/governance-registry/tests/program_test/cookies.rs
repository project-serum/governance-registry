use solana_program::pubkey::*;
use solana_sdk::signature::Keypair;

#[derive(Copy, Clone)]
pub struct MintCookie {
    pub index: usize,
    pub decimals: u8,
    pub unit: f64,
    pub base_lot: f64,
    pub quote_lot: f64,
    pub pubkey: Option<Pubkey>,
}

pub struct UserCookie {
    pub key: Keypair,
    pub token_accounts: Vec<Pubkey>,
}
