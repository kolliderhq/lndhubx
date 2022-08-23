#[derive(Debug, Clone, Copy)]
pub enum LndConnectorError {
    FailedToCreateInvoice,
    FailedToSendPayment,
    FailedToGetNodeInfo,
    FailedToDecodePaymentRequest,
    FailedToQueryRoutes,
}

impl std::fmt::Display for LndConnectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
