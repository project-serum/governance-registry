use anchor_lang::prelude::*;

#[error]
pub enum ErrorCode {
    #[msg("Exchange rate must be greater than zero")]
    InvalidRate,
    #[msg("")]
    RatesFull,
    #[msg("")]
    ExchangeRateEntryNotFound,
    #[msg("")]
    DepositEntryNotFound,
    #[msg("")]
    DepositEntryFull,
    #[msg("")]
    VotingTokenNonZero,
    #[msg("")]
    InvalidDepositId,
    #[msg("")]
    InsufficientVestedTokens,
    #[msg("")]
    UnableToConvert,
    #[msg("")]
    InvalidLockupPeriod,
    #[msg("")]
    InvalidEndTs,
    #[msg("")]
    InvalidDays,
    #[msg("")]
    RateNotZero,
    #[msg("")]
    InvalidIndex,
}
