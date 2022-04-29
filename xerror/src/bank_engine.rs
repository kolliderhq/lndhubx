#[derive(Debug, Clone, Copy)]
pub enum BankError {
    UserAccountNotFound,
    AccountNotFound,
    UserAccountAlreadyExists,
    FailedTransaction,
    SwapError,
}

#[derive(Debug, Clone, Copy)]
pub enum TransactionError {
    InsufficientFunds,
}
