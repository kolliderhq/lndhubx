#[derive(Debug, Clone, Copy)]
pub enum LndConnectorError {
    FailedToCreateInvoice,
    FailedToSendPayment,
    FailedToGetNodeInfo,
}
