pub mod api;
pub mod bank_engine;
pub mod lnd_connector;
pub mod kollider_client;
pub mod dealer;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
