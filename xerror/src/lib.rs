pub mod api;
pub mod bank_engine;
pub mod dealer;
pub mod kollider_client;
pub mod lnd_connector;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
