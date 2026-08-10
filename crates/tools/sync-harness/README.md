# Sync Harness - Testing Tool

CLI tool for testing and benchmarking the Pirate Chain sync engine.

## Features

- **Full Sync**: Test complete sync from birthday to tip
- **Benchmark**: Measure sync performance over multiple runs
- **Transport Benchmark**: Compare one continuous compact-block stream with short same-endpoint streams
- **Interrupt Test**: Verify interrupt/resume behavior
- **Rollback Test**: Verify checkpoint creation and rollback

## Usage

```bash
# Full sync from birthday
cargo run --bin sync-harness -- full-sync --birthday 3800000

# Full sync to specific target height
cargo run --bin sync-harness -- full-sync --birthday 3800000 --target 4000000

# Benchmark sync performance
cargo run --bin sync-harness -- benchmark --start 4000000 --blocks 10000 --runs 3

# Compare same-endpoint compact-block transport strategies
cargo run --bin sync-harness --release -- transport-benchmark --start 4000000 --blocks 4000 --chunk-size 1000 --concurrency 2 --runs 3

# Test interrupt and resume
cargo run --bin sync-harness -- interrupt-test --birthday 4000000 --interrupt-after 5

# Test checkpoint rollback
cargo run --bin sync-harness -- rollback-test --birthday 4000000 --checkpoint-interval 5000
```

## Examples

### Full Sync with Progress

```bash
cargo run --bin sync-harness -- full-sync \
  --endpoint http://64.23.167.130:9067 \
  --birthday 3800000
```

Output includes progress lines with percent, blocks/sec, and ETA.

### Performance Benchmark

```bash
cargo run --bin sync-harness -- benchmark \
  --start 4000000 \
  --blocks 10000 \
  --runs 5
```

Output includes per-run timing plus aggregate averages.

### Transport Benchmark

```bash
cargo run --bin sync-harness --release -- transport-benchmark \
  --endpoint http://127.0.0.1:9067 \
  --start 4000000 \
  --blocks 12000 \
  --chunk-size 1000 \
  --concurrency 2 \
  --runs 3
```

Fetches the same validated height range using sequential short streams,
concurrent short streams, and one continuous stream. Run order rotates between
iterations to reduce server-cache bias. The benchmark does not read or write
wallet state.

### Interrupt Test

```bash
cargo run --bin sync-harness -- interrupt-test \
  --birthday 4000000 \
  --interrupt-after 5
```

Simulates interruption after 5 seconds and verifies graceful shutdown.

### Checkpoint Rollback

```bash
cargo run --bin sync-harness -- rollback-test \
  --birthday 4000000 \
  --checkpoint-interval 5000
```

Creates checkpoints every 5,000 blocks and verifies rollback capability.

## Logging

Set log level with `RUST_LOG`:

```bash
RUST_LOG=debug cargo run --bin sync-harness -- full-sync --birthday 4000000
```

Log levels:
- `error`: Errors only
- `warn`: Warnings and errors
- `info`: General information (default)
- `debug`: Detailed debug information
- `trace`: Very verbose tracing

## Performance Testing

For accurate performance testing:

1. **Warm up**: Run sync once to warm caches
2. **Multiple runs**: Use `--runs 5` or more for benchmarks
3. **Consistent environment**: Close other applications
4. **Network**: Use reliable connection or local lightwalletd
5. **Monitoring**: Monitor CPU/memory/network during tests

## Privacy Testing

The harness respects privacy settings:

- All requests route through configured tunnel (Tor/I2P/SOCKS5)
- No clearnet leaks
- TLS pinning enforced
- DoH for DNS resolution (system fallback for direct mode)

Test with different tunnel modes:

```bash
# With Tor (default)
TUNNEL_MODE=tor cargo run --bin sync-harness -- full-sync --birthday 4000000

# With SOCKS5
TUNNEL_MODE=socks5 SOCKS_URL=socks5://127.0.0.1:9050 cargo run --bin sync-harness -- full-sync --birthday 4000000

# Direct (for testing only)
TUNNEL_MODE=direct cargo run --bin sync-harness -- full-sync --birthday 4000000
```

## Integration with CI

Example CI usage:

```yaml
- name: Sync Performance Test
  run: |
    cargo run --bin sync-harness -- benchmark \
      --start 4000000 \
      --blocks 1000 \
      --runs 3
```

## Troubleshooting

**Connection timeout**:
- Check endpoint is reachable
- Verify firewall settings
- Ensure Tor is running if using Tor mode

**Slow sync**:
- Check network bandwidth
- Verify lightwalletd is responsive
- Consider increasing `--max-parallel-decrypt`

**Out of memory**:
- Reduce batch size
- Reduce parallel workers
- Check for memory leaks with `valgrind` or `heaptrack`



