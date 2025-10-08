# Rust Intraday MACD POC

A high-performance intraday trading analysis system built in Rust, featuring real-time MACD indicator calculations and web-based visualization.

## Features

- **Real-time Data Processing**: Efficient handling of tick data with SQLite and Redis
- **MACD Indicator**: Streaming EMA and MACD calculations for technical analysis
- **Web API**: RESTful API with Actix-web for data access and visualization
- **Multi-source Data**: Support for EastMoney, Baidu, and Sina data sources
- **Containerized**: Docker support for easy deployment
- **Configurable**: Flexible configuration system with environment support
- **Production Ready**: Comprehensive error handling, logging, and monitoring

## Architecture

The system follows a modular architecture:

```
src/
├── main.rs          # Application entry point
├── config.rs        # Configuration management
├── storage.rs       # Data persistence layer (SQLite + Redis)
├── indicators.rs    # Technical indicators (EMA, MACD)
├── app.rs           # Business logic and trading operations
├── web.rs           # Web API endpoints
├── error.rs         # Error handling and types
└── tests.rs         # Unit tests
```

### Key Components

- **TradingApp**: Core business logic orchestrator
- **Storage**: Dual-layer persistence (SQLite for historical, Redis for real-time)
- **MACDCalc**: Streaming MACD indicator implementation
- **Web API**: REST endpoints for data access and analysis

## Quick Start

### Prerequisites

- Rust 1.70+
- Redis server
- SQLite

### Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd rust-intraday-macd-poc
```

2. Build the project:
```bash
make build
```

3. Generate sample data:
```bash
make db-reset
```

4. Run the application:
```bash
make run
```

### Using Docker

```bash
# Start all services
make docker-up

# View logs
make docker-logs

# Stop services
make docker-down
```

## Configuration

The application supports multiple environments through configuration files:

- `config/default.toml` - Base configuration
- `config/test.toml` - Test environment
- Environment variables override file settings

Set environment variables:
```bash
export RUN_MODE=production
export RUST_LOG=info
```

## API Endpoints

### Health & Status
- `GET /api/health` - Health check
- `GET /api/status` - Application status
- `GET /api/get_mode` - Get current run mode
- `POST /api/set_mode/{mode}` - Set run mode (sim/real)

### Data Access
- `GET /api/latest/{symbol}` - Latest tick for symbol
- `GET /api/trades/{symbol}` - Latest trade for symbol
- `GET /api/symbols` - List all available symbols
- `GET /api/history/{symbol}` - Historical data with MACD analysis

### Examples

```bash
# Get application status
curl http://localhost:8080/api/status

# Get latest tick for a symbol
curl http://localhost:8080/api/latest/600733.SH

# Get historical analysis
curl "http://localhost:8080/api/history/600733.SH?date=2024-01-15"
```

## Development

### Common Tasks

```bash
# Format code
make fmt

# Run linter
make lint

# Run tests
make test

# Development workflow
make dev

# Watch for changes
make watch-run
```

### Testing

```bash
# Run all tests
make test

# Run with verbose output
make test-verbose

# Run with coverage
make test-coverage
```

## Data Sources

The system supports multiple data sources:

- **EastMoney**: Comprehensive Chinese market data
- **Baidu**: Alternative data source
- **Sina**: Real-time market data
- **Simulation**: Mock data for testing

Configure data sources in `config/default.toml`:

```toml
[data_source.eastmoney]
enabled = true
base_url = "http://push2.eastmoney.com"

[data_source.baidu]
enabled = true
base_url = "https://finance.pae.baidu.com"

[data_source.sina]
enabled = true
base_url = "https://hq.sinajs.cn"
```

## Performance

- **Low Latency**: Optimized SQLite with WAL mode
- **High Throughput**: Async/await architecture with Tokio
- **Memory Efficient**: Streaming indicator calculations
- **Scalable**: Connection pooling and batch operations

## Monitoring

The application includes comprehensive logging and health checks:

```bash
# Check health
curl http://localhost:8080/api/health

# View logs
tail -f logs/app.log
```

## Deployment

### Production Deployment

1. Build release binary:
```bash
make release
```

2. Or use Docker:
```bash
make release-docker
```

3. Deploy with environment configuration:
```bash
export RUN_MODE=production
export RUST_LOG=info
./target/release/rust-intraday-macd-poc
```

### Environment Variables

- `RUN_MODE`: Application mode (development/test/production)
- `RUST_LOG`: Logging level (debug/info/warn/error)
- `APP_DATABASE__SQLITE_PATH`: Override SQLite database path
- `APP_DATABASE__REDIS_URL`: Override Redis connection URL

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make changes and run tests
4. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Support

For issues and questions:
- Create an issue in the repository
- Check the documentation
- Review the test cases for usage examples