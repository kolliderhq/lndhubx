# 🏦 LndHubX ⚡
 
##### A lightning bank with fiat accounts
LndhubX is a reimplementation of [LndHub](https://github.com/BlueWallet/LndHub) originally created by [BlueWallet](https://bluewallet.io/). This project is meant to be an improvement as well as an enhancement of the original one. The focus lies on the following areas:
 
🦀 Use of a **strongly type programming language (Rust)** to ensure type safety and performance.
 
💾 Use of a **scalable relational database** to persist account data and transactional data (PSQL)
 
📒 Use of **double entry accounting** to ensure a transparent and fault tolerant accounting trail.
 
💶 Support of **synthetic fiat accounts** and payments by integrating directly with Kollider.

### Overview
The project consists of 3 microservices.
 
#### Api
A simple REST api that allows users to interface with the bank. The api is stateless and uses JWT tokens for authentication.
 
#### Bank
The bank is the central ledger of the project. It keeps track of account balances, transactions and connects to the lightning node.
 
#### Dealer
The dealer is an OTC desk that works on the behalf of the bank to hedge the bank's fiat exposure. We implemented an RFQ system between the Dealer and the Bank in order for the dealer to manage risk levels as well as for the user to see an exchange rate before making a currency swap.

Currently supported fiat currencies:
- [x] USD 💵
- [ ] EUR 💶
- [ ] GBP 💷
- [ ] ?
 
---
 
### Known issues
- No integration tests.
- Blocking on making a lightning payment. Which allows anyone to ddos the bank with a Hodl invoice.
---
### TODO
- Add spread configuration to the dealer RFQ system to charge users for currency conversions.
- Insurance fund for the dealer. 
- Add more parameters to the Dealer to improve its strategy.
- No prevention or monitoring of fee ransom attacks.
- Add logging
- API rate limiting
- Add a fee model to the bank so the bank can charge transaction fees on top of the network fees.
- Add lnurl auth. For users as well as the Dealer to login over lnurl auth rather than API Keys.
- Admin dashboard to manage accounts.
---
 
### How to run PROD
The easiest way to launch the project is by copying your `tls.cert` and `admin.macaroon` files to the `config` dir
```
cp /path/to/tls.cert config/
cp /path/to/admin.macaroon config/
```
in the root directory. After you set all necessary config variables in the `lndhubx.prod.toml` file you can launch lndhubx by running

```
./start.sh
```
This wraps docker-compose and will bring up the DB, create the database if needed, and start up the services.

### How to run DEV
Each microservice can be run individually
 
```
ENV=dev FILE_NAME=lndhubx cargo run --bin api
ENV=dev FILE_NAME=lndhubx cargo run --bin dealer
ENV=dev FILE_NAME=lndhubx cargo run --bin bank
```
-----------
 
### Tests
To run the test you can run
```
cargo test
```
------

 
### Configuration
 
`psql_url` = The postgres url. <br/>
`tls_path` = Path to the tls certificate of the lnd node. <br/>
`macaroon_path` = Path to the admin.macaroon file of your lnd node. <br>
`node_url` = The url of your lnd node. <br>
`kollider_ws_url` = "wss:/api.kollider.xyz/v1/ws/" <br>
`kollider_api_key` = Your Kollider api key. <br>
`kollider_api_secret` = Your Kollider secret. <br>
`kollider_api_passphrase` = Your Kollider passphrase. <br>


##### Dealer config
It's optional to run the dealer and if you don't specify your API keys in the `lndhubx.prod.toml` then the service will simply exit. In order to get api keys you have to first register on [Kollider](https://pro.kollider.xyz). Then navigate to https://pro.kollider.xyz/dashboard/developer. Ther you can generate a fresh set of API keys. Make sure you select the `Trade` `View` and `Transfer` permissions, otherwise the Dealer won't be able to perform its magic.

 --------
 
### Synthetic Fiat Accounts
LndhubX natively supports synthetic fiat accounts if it is run with the Dealer service. This is achieved by hedging the banks fiat exposure through taking a short position in an inversely price perpetual swap. If you want to learn more about how this exactly works we published a blog post on this [here]().

### 🚧 🚨 **Under construction** 🚨🚧
 The project is for demo purposes at the moment but PR's to improve it are welcome! 
 