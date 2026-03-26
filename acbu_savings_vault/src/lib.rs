#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol, Vec};


#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub acbu_token: Symbol,
    pub fee_rate: Symbol,
    pub yield_rate: Symbol,
    pub paused: Symbol,
    pub version: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    acbu_token: symbol_short!("ACBU_TKN"),
    fee_rate: symbol_short!("FEE_RATE"),
    yield_rate: symbol_short!("YLD_RATE"),
    paused: symbol_short!("PAUSED"),
    version: symbol_short!("VERSION"),
};

const DEPOSIT_KEY: Symbol = symbol_short!("DEPOSITS");
const SECONDS_PER_YEAR: i128 = 31_536_000;
const VERSION: u32 = 1;


#[contracttype]
#[derive(Clone, Debug)]
pub struct DepositLot {
    pub amount: i128,
    pub timestamp: u64,
    pub term_seconds: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct DepositEvent {
    pub user: Address,
    pub amount: i128,
    pub term_seconds: u64,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct WithdrawEvent {
    pub user: Address,
    pub amount: i128,
    pub fee_amount: i128,
    pub yield_amount: i128,
    pub timestamp: u64,
}

#[contract]
pub struct SavingsVault;

#[contractimpl]
impl SavingsVault {
    /// Initialize the savings vault contract
    pub fn initialize(
        env: Env,
        admin: Address,
        acbu_token: Address,
        fee_rate_bps: i128,
        yield_rate_bps: i128,
    ) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            panic!("Contract already initialized");
        }
        if !(0..=10_000).contains(&fee_rate_bps) {
            panic!("Invalid fee rate");
        }
        if !(0..=10_000).contains(&yield_rate_bps) {
            panic!("Invalid yield rate");
        }
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage()
            .instance()
            .set(&DATA_KEY.yield_rate, &yield_rate_bps);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage().instance().set(&DATA_KEY.version, &VERSION);
    }

    /// Deposit (lock) ACBU for a term. User transfers ACBU to this contract.
    pub fn deposit(
        env: Env,
        user: Address,
        amount: i128,
        term_seconds: u64,
    ) -> Result<i128, soroban_sdk::Error> {
        user.require_auth();
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(1001));
        }
        if amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(1002));
        }
        if term_seconds == 0 {
            return Err(soroban_sdk::Error::from_contract_error(1007));
        }

        let acbu: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let client = soroban_sdk::token::Client::new(&env, &acbu);
        client.transfer(&user, &env.current_contract_address(), &amount);

        let key = (DEPOSIT_KEY, user.clone(), term_seconds);
        let mut lots: Vec<DepositLot> = env
            .storage()
            .temporary()
            .get(&key)
            .unwrap_or(Vec::new(&env));
        lots.push_back(DepositLot {
            amount,
            timestamp: env.ledger().timestamp(),
            term_seconds,
        });
        env.storage().temporary().set(&key, &lots);

        env.events().publish(
            (symbol_short!("Deposit"), user.clone()),
            DepositEvent {
                user,
                amount,
                term_seconds,
                timestamp: env.ledger().timestamp(),
            },
        );
        Ok(Self::sum_lots(&lots))
    }

    /// Withdraw (unlock) ACBU after term. Applies the stored protocol fee.
    pub fn withdraw(
        env: Env,
        user: Address,
        term_seconds: u64,
        amount: i128,
    ) -> Result<(), soroban_sdk::Error> {
        // Auth first
        user.require_auth();
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(1001));
        }
        if amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(1002));
        }
        let key = (DEPOSIT_KEY, user.clone(), term_seconds);
        let lots: Vec<DepositLot> = env
            .storage()
            .temporary()
            .get(&key)
            .ok_or(soroban_sdk::Error::from_contract_error(1003))?;
        let now = env.ledger().timestamp();
        // Check that at least one lot has matured; compute total unlocked balance
        let unlocked_balance: i128 = lots
            .iter()
            .filter(|lot| now >= lot.timestamp.saturating_add(lot.term_seconds))
            .fold(0i128, |acc, lot| acc + lot.amount);
        if unlocked_balance < amount {
            // Some lots may still be locked
            return Err(soroban_sdk::Error::from_contract_error(1006));
        }

        let fee_rate: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.fee_rate)
            .unwrap_or(0);
        let yield_rate: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.yield_rate)
            .unwrap_or(0);
        let fee_amount: i128 = (amount * fee_rate) / 10_000;
        let mut amount_left = amount;
        let mut updated_lots = Vec::new(&env);
        let mut yield_amount: i128 = 0;

        for lot in lots.iter() {
            if amount_left == 0 {
                updated_lots.push_back(lot);
                continue;
            }
            // Only consume lots whose lock term has elapsed
            let unlocked = now >= lot.timestamp.saturating_add(lot.term_seconds);
            if !unlocked {
                // Term not yet elapsed — keep this lot intact
                updated_lots.push_back(lot);
                continue;
            }

            if lot.amount <= amount_left {
                amount_left -= lot.amount;
                let elapsed_seconds = now.saturating_sub(lot.timestamp);
                yield_amount += Self::calculate_yield(lot.amount, yield_rate, elapsed_seconds)?;
            } else {
                let consumed = amount_left;
                let remaining = lot.amount - consumed;
                let elapsed_seconds = now.saturating_sub(lot.timestamp);
                yield_amount += Self::calculate_yield(consumed, yield_rate, elapsed_seconds)?;

                updated_lots.push_back(DepositLot {
                    amount: remaining,
                    timestamp: lot.timestamp,
                    term_seconds: lot.term_seconds,
                });
                amount_left = 0;
            }
        }

        if amount_left > 0 {
            return Err(soroban_sdk::Error::from_contract_error(1004));
        }

        if updated_lots.is_empty() {
            env.storage().temporary().remove(&key);
        } else {
            env.storage().temporary().set(&key, &updated_lots);
        }

        let net_amount: i128 = amount - fee_amount;
        let payout_amount: i128 = net_amount + yield_amount;

        let acbu: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let client = soroban_sdk::token::Client::new(&env, &acbu);
        client.transfer(&env.current_contract_address(), &user, &payout_amount);
        if fee_amount > 0 {
            let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
            client.transfer(&env.current_contract_address(), &admin, &fee_amount);
        }

        env.events().publish(
            (symbol_short!("Withdraw"), user.clone()),
            WithdrawEvent {
                user,
                amount,
                fee_amount,
                yield_amount,
                timestamp: env.ledger().timestamp(),
            },
        );
        Ok(())
    }

    /// Get balance for user and term
    pub fn get_balance(env: Env, user: Address, term_seconds: u64) -> i128 {
        let key = (DEPOSIT_KEY, user, term_seconds);
        let lots: Vec<DepositLot> = env
            .storage()
            .temporary()
            .get(&key)
            .unwrap_or(Vec::new(&env));
        Self::sum_lots(&lots)
    }

    fn sum_lots(lots: &Vec<DepositLot>) -> i128 {
        let mut total = 0;
        for lot in lots.iter() {
            total += lot.amount;
        }
        total
    }

    fn calculate_yield(
        principal: i128,
        yield_rate_bps: i128,
        elapsed_seconds: u64,
    ) -> Result<i128, soroban_sdk::Error> {
        let elapsed_i128 = i128::from(elapsed_seconds);
        let numerator = principal
            .checked_mul(yield_rate_bps)
            .and_then(|v| v.checked_mul(elapsed_i128))
            .ok_or(soroban_sdk::Error::from_contract_error(1005))?;

        Ok(numerator / (10_000 * SECONDS_PER_YEAR))
    }

    pub fn pause(env: Env) -> Result<(), soroban_sdk::Error> {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &true);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), soroban_sdk::Error> {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &false);
        Ok(())
    }

    pub fn version(_env: Env) -> u32 {
        VERSION
    }

    pub fn migrate(env: Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();

        let current_version = VERSION;
        let stored_version: u32 = env.storage().instance().get(&DATA_KEY.version).unwrap_or(0);
        if stored_version < current_version {
            env.storage()
                .instance()
                .set(&DATA_KEY.version, &current_version);
        }
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }
}

