#[derive(Debug, Clone, Copy)]
pub enum BankError {
    UserAccountNotFound,
    AccountNotFound,
    UserAccountAlreadyExists,
    FailedTransaction,
    UserSuspended,
    SwapError,
}

impl std::fmt::Display for BankError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let output = match self {
            BankError::UserAccountNotFound => "UserAccountNotFound",
            BankError::AccountNotFound => "AccountNotFound",
            BankError::UserAccountAlreadyExists => "UserAccountAlreadyExists",
            BankError::FailedTransaction => "FailedTransaction",
            BankError::UserSuspended => "UserSuspended",
            BankError::SwapError => "SwapError",
        };
        write!(f, "{}", output)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TransactionError {
    InsufficientFunds,
}
