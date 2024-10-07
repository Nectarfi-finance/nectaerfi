use anchor_lang::prelude::*;
use anchor_spl::token::{self, Burn, Mint, Token, TokenAccount, Transfer, MintTo};
use anchor_spl::associated_token::AssociatedToken;
use std::collections::HashMap;

declare_id!("NECTMRLbg1N5H66peinv7Yfau8183Y8RPSoAEHc8ErE");

#[program]
pub mod nectarfi {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let nectarfi_state = &mut ctx.accounts.nectarfi_state;
        nectarfi_state.last_yield_check = Clock::get()?.unix_timestamp;
        nectarfi_state.current_best_yield = 0;
        nectarfi_state.total_deposits = 0;
        nectarfi_state.current_best_protocol = "None".to_string();
        nectarfi_state.nct_mint = ctx.accounts.nct_mint.key();
        Ok(())
    }

   pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
    // Transfer USDC from user to vault
    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.user_token_account.to_account_info(),
                to: ctx.accounts.vault_token_account.to_account_info(),
                authority: ctx.accounts.user.to_account_info(),
            },
        ),
        amount,
    )?;

    // Calculate amount of NCT tokens to mint
    let nct_to_mint = if ctx.accounts.nectarfi_state.total_deposits == 0 {
        amount
    } else {
        amount * ctx.accounts.nct_mint.supply / ctx.accounts.nectarfi_state.total_deposits
    };

    // Mint new NCT tokens
    token::mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            MintTo {
                mint: ctx.accounts.nct_mint.to_account_info(),
                to: ctx.accounts.user_nct_account.to_account_info(),
                authority: ctx.accounts.nectarfi_state.to_account_info(),
            },
            &[&[b"nectar_acct", &[ctx.bumps.nectarfi_state]]],
        ),
        nct_to_mint,
    )?;

    // Update total deposits
    ctx.accounts.nectarfi_state.total_deposits += amount;

    Ok(())
}

    pub fn withdraw(ctx: Context<Withdraw>, nct_amount: u64) -> Result<()> {
    let total_deposits = ctx.accounts.nectarfi_state.total_deposits;
    let nct_supply = ctx.accounts.nct_mint.supply;

    // Calculate USDC amount to return
    let usdc_to_return = (nct_amount as u128)
        .checked_mul(total_deposits as u128)
        .unwrap()
        .checked_div(nct_supply as u128)
        .unwrap() as u64;

    // Burn NCT tokens
    token::burn(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Burn {
                mint: ctx.accounts.nct_mint.to_account_info(),
                from: ctx.accounts.user_nct_account.to_account_info(),
                authority: ctx.accounts.user.to_account_info(),
            },
        ),
        nct_amount,
    )?;

    // Transfer USDC from vault to user
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.vault_token_account.to_account_info(),
                to: ctx.accounts.user_token_account.to_account_info(),
                authority: ctx.accounts.nectarfi_state.to_account_info(),
            },
            &[&[b"nectar_acct", &[ctx.bumps.nectarfi_state]]],
        ),
        usdc_to_return,
    )?;

    // Update total deposits
    let nectarfi_state = &mut ctx.accounts.nectarfi_state;
    nectarfi_state.total_deposits = nectarfi_state.total_deposits.checked_sub(usdc_to_return).unwrap();

    Ok(())
}

    pub fn check_yields(mut ctx: Context<CheckYields>) -> Result<()> {
        let current_timestamp = Clock::get()?.unix_timestamp;

        {
            let nectarfi_state = &mut ctx.accounts.nectarfi_state;
            if current_timestamp - nectarfi_state.last_yield_check < 300 {
                return Ok(());
            }
        }

        let yields = fetch_current_yields();
        let best_yield = yields.values().cloned().max().unwrap_or(0);

        if best_yield > ctx.accounts.nectarfi_state.current_best_yield {
            rebalance_funds(&mut ctx, &yields)?;
            ctx.accounts.nectarfi_state.current_best_yield = best_yield;
        }

        ctx.accounts.nectarfi_state.last_yield_check = current_timestamp;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = user, space = 8 + 8 + 8 + 8 + 32 + 32, seeds = [b"nectar_acct"], bump)]
    pub nectarfi_state: Account<'info, NectarfiState>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        init,
        payer = user,
        mint::decimals = 6,
        mint::authority = nectarfi_state,
    )]
    pub nct_mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut, seeds = [b"nectar_acct"], bump)]
    pub nectarfi_state: Account<'info, NectarfiState>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = usdc_mint,
        associated_token::authority = nectarfi_state
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    pub usdc_mint: Account<'info, Mint>,
    #[account(mut, address = nectarfi_state.nct_mint)]
    pub nct_mint: Account<'info, Mint>,
    #[account(mut)]
    pub user_nct_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut, seeds = [b"nectar_acct"], bump)]
    pub nectarfi_state: Account<'info, NectarfiState>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_token_account: Account<'info, TokenAccount>,
    pub usdc_mint: Account<'info, Mint>,
    #[account(mut, address = nectarfi_state.nct_mint)]
    pub nct_mint: Account<'info, Mint>,
    #[account(mut)]
    pub user_nct_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CheckYields<'info> {
    #[account(mut)]
    pub nectarfi_state: Account<'info, NectarfiState>,
    /// CHECK: This is safe because we only read from this account
    pub clock: UncheckedAccount<'info>,
}

#[account]
pub struct NectarfiState {
    pub last_yield_check: i64,
    pub current_best_yield: u64,
    pub total_deposits: u64,
    pub current_best_protocol: String,
    pub nct_mint: Pubkey,
}

fn fetch_current_yields() -> HashMap<String, u64> {
    let mut yields = HashMap::new();
    yields.insert("ProtocolA".to_string(), 500);
    yields.insert("ProtocolB".to_string(), 550);
    yields.insert("ProtocolC".to_string(), 480);
    yields
}

fn rebalance_funds(ctx: &mut Context<CheckYields>, yields: &HashMap<String, u64>) -> Result<()> {
    msg!("Rebalancing funds");
    let (best_protocol, best_yield) = yields
        .iter()
        .max_by_key(|&(_, yield_value)| yield_value)
        .ok_or(ProgramError::InvalidAccountData)?;

    msg!(
        "Rebalancing funds to {} with yield of {}%",
        best_protocol,
        best_yield / 100
    );

    let nectarfi_state = &mut ctx.accounts.nectarfi_state;

    nectarfi_state.current_best_yield = *best_yield;
    nectarfi_state.current_best_protocol = best_protocol.clone();

    let transfer_fee = nectarfi_state.total_deposits / 1000;
    nectarfi_state.total_deposits -= transfer_fee;

    msg!(
        "Transferred {} to {}",
        nectarfi_state.total_deposits,
        best_protocol
    );
    msg!("Transfer fee: {}", transfer_fee);

    emit!(RebalanceEvent {
        timestamp: Clock::get()?.unix_timestamp,
        new_protocol: best_protocol.clone(),
        new_yield: *best_yield,
        total_balance: nectarfi_state.total_deposits,
    });

    Ok(())
}

#[event]
pub struct RebalanceEvent {
    pub timestamp: i64,
    pub new_protocol: String,
    pub new_yield: u64,
    pub total_balance: u64,
}
