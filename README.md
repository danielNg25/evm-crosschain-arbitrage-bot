# EVM Cross-Chain Arbitrage Bot

A high-performance Rust-based arbitrage bot that identifies and executes profitable cross-chain trading opportunities across multiple EVM-compatible blockchains and DEX protocols (Uniswap V2, V3, Aerodrome, etc.).

## 🚀 Features

-   **Cross-Chain Arbitrage**: Automatically detects and executes arbitrage opportunities across multiple blockchain networks
-   **Multi-DEX Support**: Supports Uniswap V2, Uniswap V3, Aerodrome, and other DEX protocols
-   **Real-Time Monitoring**: Listens to swap events via WebSocket and HTTP polling for instant opportunity detection
-   **MongoDB Integration**: Persistent storage for networks, pools, tokens, paths, and configuration
-   **Price Tracking**: Integrates with GeckoTerminal and other price sources for accurate profit calculations
-   **Telegram Notifications**: Real-time alerts and logging via Telegram bot
-   **Configurable**: Dynamic configuration management with MongoDB-backed settings
-   **High Performance**: Built with Rust for maximum performance and low latency
-   **Metrics & Monitoring**: Built-in metrics tracking for performance analysis

## 📋 Prerequisites

-   **Rust** (latest stable version, 2021 edition)
-   **MongoDB** (3.2+)
-   **Node.js** (for contract compilation, if needed)
-   **Access to blockchain RPC endpoints** for the networks you want to monitor

## 🛠️ Installation

1. **Clone the repository**

    ```bash
    git clone <repository-url>
    cd evm-crosschain-arbitrage
    ```

2. **Build the project**

    ```bash
    cargo build --release
    ```

3. **Set up configuration**

    ```bash
    cp configs/config.example.toml configs/config.toml
    # Edit config.toml with your settings
    ```

4. **Set up MongoDB**
    - Ensure MongoDB is running and accessible
    - The bot will automatically create the necessary collections

## ⚙️ Configuration

Edit `configs/config.toml` with your settings:

```toml
[mongodb]
database = "crosschain"
uri = "mongodb://localhost:27017"  # Your MongoDB connection string
poll_interval_secs = 60             # How often to poll MongoDB for updates

[processor]
max_iterations = 200                 # Maximum iterations for binary search
max_amount_usd = 20000.0            # Maximum USD amount to swap (default, can be overridden in DB)

[executor]
default_recheck_interval = 60        # Default interval to recheck opportunities (seconds)

[telegram]
token = "YOUR_TELEGRAM_BOT_TOKEN"    # Optional: Telegram bot token
chat_id = "YOUR_CHAT_ID"             # Optional: Telegram chat ID for notifications
```

### MongoDB Collections

The bot uses the following MongoDB collections:

-   `networks` - Blockchain network configurations
-   `pools` - DEX pool addresses and metadata
-   `tokens` - Token information (address, symbol, decimals)
-   `paths` - Cross-chain arbitrage paths
-   `configs` - Application configuration (singleton)

## 🎯 Usage

### Basic Usage

```bash
# Run with default settings
cargo run --release

# Run with custom log level
cargo run --release -- --log-level debug

# Run with specific strategy
cargo run --release -- --strategy aggressive
```

### Command Line Arguments

-   `--log-level <level>`: Set logging level (error, warn, info, debug, trace). Default: `info`
-   `--strategy <strategy>`: Trading strategy to use. Default: `default`

## 🏗️ Architecture

The bot is organized into several key components:

### Core Components

1. **Network Collector** (`src/core/collector/`)

    - Monitors multiple blockchain networks
    - Polls MongoDB for network/pool/token updates
    - Listens to swap events via WebSocket
    - Maintains network registries

2. **Processor** (`src/core/processor/`)

    - Analyzes swap events
    - Identifies arbitrage opportunities
    - Calculates optimal trade amounts using binary search
    - Filters opportunities based on profitability

3. **Executor** (`src/core/executor/`)

    - Executes identified arbitrage opportunities
    - Manages transaction execution
    - Handles rechecking of opportunities
    - Logs execution results

4. **Models** (`src/models/`)
    - Pool models (V2, V3 implementations)
    - Token registry
    - Path registry for cross-chain routes
    - Profit token tracking with price sources

### Services

1. **Database Service** (`src/services/database/`)

    - MongoDB integration
    - Repository pattern for data access
    - Models for networks, pools, tokens, paths, config

2. **Telegram Service** (`src/services/telegram/`)
    - Telegram bot integration
    - Notification and logging capabilities

## 📁 Project Structure

```
evm-crosschain-arbitrage/
├── bin/
│   └── main.rs                 # Application entry point
├── configs/
│   ├── config.example.toml     # Example configuration
│   └── config.toml             # Your configuration (gitignored)
├── contracts/                  # Solidity contracts
├── src/
│   ├── blockchain/             # Blockchain interaction utilities
│   ├── config/                 # Configuration management
│   ├── core/                   # Core bot logic
│   │   ├── collector/         # Network and event collection
│   │   ├── executor/           # Opportunity execution
│   │   └── processor/          # Arbitrage opportunity processing
│   ├── models/                 # Data models
│   │   ├── path/              # Cross-chain path models
│   │   ├── pool/               # DEX pool models (V2, V3)
│   │   ├── profit_token/       # Profit token tracking
│   │   └── token/              # Token models
│   ├── services/               # External services
│   │   ├── database/           # MongoDB service
│   │   └── telegram/           # Telegram service
│   └── utils/                  # Utility functions
└── Cargo.toml
```

## 🔑 Key Features Explained

### Cross-Chain Arbitrage Detection

The bot monitors swap events across multiple chains and identifies price discrepancies that can be exploited through cross-chain arbitrage paths.

### Dynamic Configuration

Configuration is stored in MongoDB, allowing runtime updates without restarting the bot:

-   `max_amount_usd`: Maximum USD value to trade per opportunity
-   `recheck_interval`: How often to recheck opportunities before execution

### Multi-Protocol Support

Supports various DEX protocols:

-   **Uniswap V2**: Constant product AMM
-   **Uniswap V3**: Concentrated liquidity AMM
-   **Aerodrome**: Velodrome fork on Base

### Price Sources

Integrates with multiple price sources:

-   GeckoTerminal API
-   Custom price updaters

## 🔧 Development

### Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release
```

### Testing

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture
```

### Code Formatting

```bash
cargo fmt
```

### Linting

```bash
cargo clippy
```

## 📊 Monitoring

The bot includes built-in metrics tracking:

-   Opportunity detection rate
-   Execution success/failure rates
-   Profit calculations
-   Network latency

Access metrics through the `Metrics` utility in `src/utils/metrics.rs`.

## 🔒 Security Considerations

-   **Private Keys**: Never commit private keys or sensitive credentials
-   **RPC Endpoints**: Use secure, rate-limited RPC endpoints
-   **Transaction Gas**: Configure appropriate gas limits and prices
-   **Slippage**: Consider slippage protection in production

## 📝 License

[Add your license here]

## 🤝 Contributing

[Add contribution guidelines here]

## 📧 Support

[Add support/contact information here]

## 🙏 Acknowledgments

-   Built with [Alloy](https://github.com/alloy-rs/alloy) for Ethereum interactions
-   Uses [MongoDB](https://www.mongodb.com/) for data persistence
-   Inspired by the DeFi arbitrage ecosystem

---

**Note**: This bot is for educational and research purposes. Always test thoroughly on testnets before using on mainnet. Trading cryptocurrencies involves risk.
