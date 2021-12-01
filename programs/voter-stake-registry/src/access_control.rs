use crate::context::*;
use crate::error::*;
use anchor_lang::prelude::*;

pub fn rate_is_empty(ctx: &Context<CreateExchangeRate>, idx: u16) -> Result<()> {
    let r = &ctx.accounts.registrar;
    require!((idx as usize) < r.rates.len(), InvalidIndex);
    require!(r.rates[idx as usize].rate == 0, RateNotZero);
    Ok(())
}
