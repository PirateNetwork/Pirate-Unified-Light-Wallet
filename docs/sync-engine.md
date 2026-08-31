# Supernova Sync Engine

Supernova is Stashi Wallet's shielded-chain sync engine. The light server supplies compact chain data; the wallet validates its order, trial-decrypts it locally, updates the Sapling and Ironwood commitment trees, and commits the resulting wallet state.

That sounds simple until it has to be fast, resumable, memory-bounded, private over Tor or I2P, and correct across a crash or reorg. Most of the work described here came from removing unnecessary waiting, decoding, copying, and database contention without weakening any of those requirements.

The result is Supernova, the fastest zk-SNARKs shielded light wallet sync engine in the world. Representative warm cache rescans sustain tens of thousands of compact blocks per second, with individual batch peaks above 100,000 blocks per second. Speed varies with the block size, wallet activity, CPU, storage, transport, and server.


## Pipeline at a glance

A bounded sync run follows this order:

1. Read one server tip and validate the active consensus branch.
2. Validate the local chain boundary and cached compact blocks.
3. Stream missing compact blocks into an ordered, byte-bounded assembler.
4. Split durable network segments into locally adaptive scan batches.
5. Trial-decrypt Sapling and Ironwood data in parallel.
6. Build immutable commitment-tree fragments in parallel.
7. Send one ordered batch to the persistence worker.
8. Update both ShardTrees, notes, spends, transactions, checkpoints, and cursors in one transaction.
9. Verify witnesses and publish progress only after the committed state is usable.

Parallel work produces immutable results. One owner performs every ordered database and tree mutation. That rule is the backbone of both the performance work and the correctness model.

## Block intake and caching

### Binary Compact-Block Cache

**In plain terms:** Cached blocks are stored in the same kind of compact binary form used on the wire instead of being converted to bulky text and parsed again on every rescan.

**Technical detail:** The current cache format uses a versioned protobuf record with an explicit magic header and a CRC32 over the exact serialized protobuf bytes. The checksum is verified before protobuf decoding, so accidental disk corruption cannot become a plausible cached block. An unreadable or malformed cached range is discarded and fetched again. Version-one protobuf and legacy JSON rows remain readable and are upgraded transactionally only after their heights, hashes, chain links, and canonical server anchor have been validated.

**Measured impact:** The protobuf codec was roughly **70 times faster** than the legacy JSON codec in the cache microbenchmark. Cache decoding was only a small part of an end-to-end sync, so this is not a claim that the whole wallet became 70 times faster. It removed a disproportionately wasteful operation from every cached rescan and reduced the cache's storage and allocation overhead at the same time.

The integrity envelope was benchmarked separately in an optimized build over 65,536 blocks. Median decoding moved from 0.135 seconds for version-one records to 0.140 seconds for checksummed version-two records: about 5 milliseconds, or 3.87%, inside the decode-only component. Scaled to the representative 583,000-block cached rescan, the measured checksum cost is roughly 45 milliseconds and remains within normal end-to-end run variance.

### Constant-Time Coverage Proof

**In plain terms:** Before reading a cached range, the wallet can usually prove that all requested blocks are present without walking every row first.

**Technical detail:** Cache metadata uses the primary-key count and minimum/maximum heights as a fast proof for the common contiguous case. Any suspicious count, boundary, or gap falls back to the exact sequential validator. The fast path changes the cost of checking a large healthy cache from work proportional to every block to a small indexed query, while the fallback preserves exact gap detection.

### Continuous Ordered Stream

**In plain terms:** The downloader keeps the pipe open instead of repeatedly connecting, downloading a chunk, stopping for local work, and starting again.

**Technical detail:** A single long-lived compact-block stream feeds an ordered assembler. Every incoming block is checked for the expected height, hash continuity, requested boundary, and encoded length. If a stream fails, the assembler keeps the validated prefix and resumes from the next uncommitted height rather than throwing away the complete range.

### Actual-Byte Bounded Assembly

**In plain terms:** Memory limits are based on the bytes actually received, not a guess that every block is the same size.

**Technical detail:** Stream chunks and local batches are bounded by encoded bytes as well as block count. A byte semaphore accounts for queued data, and an unusually large single block is handled explicitly so the pipeline cannot deadlock waiting for an impossible permit. This keeps the pipeline useful on low-memory phones without artificially starving desktop machines.

### Two-Level Batching

**In plain terms:** Downloading and scanning no longer have to move in the same size steps. The network can keep feeding data while the local device chooses the amount it can process efficiently.

**Technical detail:** Durable network segments are independent from adaptive local scan batches. Network continuity is stable and transport-facing behavior does not expose fine-grained CPU timing. The local controller can split or combine the buffered blocks based on measured processing throughput, memory pressure, and pipeline saturation without changing the server stream.

### Watermark Prefetch Reservoir

**In plain terms:** A small reservoir keeps the scanner fed, but stops downloading before queued blocks can consume too much memory.

**Technical detail:** High- and low-water marks control byte-bounded prefetch. Fetching resumes below the low watermark and pauses at the high watermark. The queue remains deliberately shallow, so cancellation and reorg recovery do not leave a long speculative tail to discard.

### Adaptive Durable Segments

**In plain terms:** The wallet can enlarge its internal segments on a fast connection and shrink them when memory, storage, or the endpoint cannot keep up.

**Technical detail:** The controller uses quantized size buckets, hysteresis, and separate network/cache observations. It reacts to sustained throughput rather than one noisy latency sample, which avoids oscillating between segment sizes. The continuous stream remains unchanged; this is an internal durability and buffering decision, not a different request fingerprint for every device.

### Validated Multi-Server Intake

**In plain terms:** Auto mode can use more than one healthy light server for old blocks, but the blocks still enter the wallet as one continuous chain. A stalled server can be replaced without silently bypassing the user's network choice.

**Technical detail:** Auto is a Stashi Wallet policy layered over an explicit core API. Existing SDK, CLI, React Native, and Qortal calls remain single-server unless the caller deliberately supplies a pool. A pool is rejected if its members cross Pirate networks, mix clearnet, onion, or I2P routes, change the connection security mode, or combine automatic failover with a pinned primary.

When Tor is selected, the wallet suggests the native onion endpoints first and then the curated TLS clearnet endpoints, which are also reachable through the active Tor transport. The automatic Tor pool remains onion-only, while a manually selected clearnet endpoint stays confined to Tor until the user changes transport. This keeps the preferred hidden services first without treating a clearnet hostname as permission to bypass Tor.

The clearnet preset starts with a subtree-root-capable CryptoForge endpoint, then retains Pirate.Black, Mathnodes, Qortal, and the second CryptoForge endpoint as canonical failover and historical-stream candidates. Health, chain identity, and tip checks still override that preference whenever the primary is unavailable or stale.

Candidates are probed through the selected Direct, Tor, SOCKS5, or I2P transport. They must agree on chain metadata and the hash at a common historical anchor before receiving work. When the selected server is available, its anchor is authoritative; otherwise a strict majority of responding candidates is required when more than one answer exists.

Direct historical sync can use up to three validated streams, while Tor and SOCKS5 use at most two. I2P remains single-stream with validated failover to avoid multiplying tunnel pressure. The final 100 blocks are read from one source so ordinary tip movement cannot produce a striped view across competing tips.

Each source receives a disjoint 256-block range. Results are buffered behind one byte semaphore, reordered by height, and checked for exact range coverage and parent-hash continuity across source boundaries. Only that validated prefix reaches the existing scanner and single persistence owner. A failed source is quarantined after bounded retries, and work resumes at the first height not yet accepted by the ordered assembler.

## Cryptographic scanning

### Prepared Viewing-Key Context

**In plain terms:** The wallet prepares expensive key material once per scan instead of rebuilding it for every block or output.

**Technical detail:** Incoming viewing keys and receiver metadata are normalized into reusable trial-decryption contexts. Sapling and Ironwood work is grouped by pool and processed in bounded chunks, reducing repeated setup and improving cache locality.

### Parallel Ordered Trial Decryption

**In plain terms:** Available CPU cores test shielded outputs in parallel, but the answers are put back into exact chain order before anything is saved.

**Technical detail:** Trial decryption runs on a bounded shared Rayon pool. Work is partitioned into immutable chunks and each result carries its source position. Results are sorted and validated against the originating block boundaries before they can reach persistence. Parallelism changes scheduling, not transaction or note order.

### Direct Diversified-Address Ownership

**In plain terms:** When the wallet finds one of its notes, it identifies the exact receiving address and remembers where that address belongs. It does not try thousands of possible addresses afterward.

**Technical detail:** Sapling and Ironwood full viewing keys reverse an owned payment address to its key scope and complete 88-bit ZIP-32 diversifier index. The ordered persistence worker stores that index with the address and links the note to the address row in the same wallet-state transaction. Legacy rows are repaired by the same ownership proof when first used. The old sequential recovery walk and its 4,096-address ceiling are not part of balance reconstruction.

**Performance impact:** Index recovery runs once per owned note, not once per compact block or possible address. Healthy cached rescans retain the same trial-decryption workload and avoid the former range-derivation fallback, so the change does not add work proportional to chain length or address history.

### One-Batch Lookahead

**In plain terms:** While batch N updates the commitment trees, the CPU can already decrypt batch N+1.

**Technical detail:** The pipeline permits exactly one speculative decrypt batch ahead. Its memory is bounded and its result is accepted only if the current batch commits and the next block boundary still matches. Cancellation, rollback, reorg, and failed-commit paths discard the speculative result. In one instrumented run, trial decryption consumed 14.84 seconds of CPU time but blocked the ordered sync path for only 31 milliseconds because almost all of it overlapped tree and persistence work.

### Deferred Transaction Enrichment

**In plain terms:** The first pass does only the work needed to discover wallet activity. Full transaction and memo details are fetched later instead of making every compact block wait.

**Technical detail:** Compact scanning records deterministic discovery state first. Full transactions, memo recovery, and optional enrichment are scheduled separately and persisted through the same ordered ownership rules. This shortens the critical path while retaining repairable, eventually complete transaction history.

### Stage-Aware CPU Scheduling

**In plain terms:** Parallel stages share the machine instead of each starting enough workers to overwhelm it.

**Technical detail:** Trial decryption and parallel tree construction use bounded worker budgets derived from a common device profile. Scheduling responds to the stage currently limiting throughput, and one-batch lookahead is curtailed when it would compete with current-batch tree work. This matters on mobile devices where oversubscription can be slower and can trigger thermal throttling.

## Commitment-tree work

### ShardTree Foundation

**In plain terms:** The wallet stores the commitment tree in small independently loadable pieces, so it does not have to keep or rewrite the whole tree for every batch.

**Technical detail:** Sapling and Ironwood commitments use `ShardTree` with retained checkpoints, marked leaves, and witness-aware pruning. Shards make durable updates local, while the checkpoint model gives rollback and witness repair an explicit historical boundary. This is why Supernova keeps ShardTree rather than returning to a faster but less suitable in-memory bridge-tree shortcut.

### Parallel Prunable-Tree Construction

**In plain terms:** Independent pieces of a large commitment range are built on several cores, then joined in order by the tree owner.

**Technical detail:** A contiguous immutable commitment batch is divided into disjoint position ranges. Each range constructs an owned `LocatedPrunableTree` in parallel using ShardTree 0.7.1's construction path. The persistence worker inserts those trees sequentially with `insert_tree`, preserving canonical positions, marks, and checkpoints while moving hashing and node construction off the serial writer path.

### Balanced Construction Ranges

**In plain terms:** Parallel tree jobs are divided by real work rather than giving one core a difficult piece while the others finish early.

**Technical detail:** Range sizing accounts for commitment density and available workers. Immutable batch preparation also happens before the sequential frontier section. The result reduces pipeline tails without allowing two workers to mutate the same tree.

### Long-Lived Sparse Shard Cache

**In plain terms:** Recently used tree pieces stay warm between batches, but the wallet keeps only the pieces it has proved it needs.

**Technical detail:** The persistence worker exclusively owns a sparse cache of validated loaded shards. Cache hits avoid repeated SQLite reads and decoding; dirty deltas are flushed inside the same transaction as their corresponding wallet state. The cache has byte limits and eviction accounting, and it is invalidated or reloaded after rollback, cancellation, failed commit, or reorg. No shared mutable tree state escapes the worker.

### Verified Subtree Grafting

**In plain terms:** A server can provide a shortcut over old history, but the wallet checks samples itself and stops using the shortcut if the answer is wrong.

**Technical detail:** Historical Sapling and Ironwood subtree roots are fetched as an optional acceleration. The wallet independently reconstructs sampled roots from compact-block commitments. A disagreement disables or bypasses grafting and replays local leaves; it does not replace verified local wallet state with an untrusted server claim. Marked or wallet-owned ranges remain leaf-backed so witnesses can still be constructed.

### Capability-Cached Root Retrieval

**In plain terms:** Auto can use one healthy server for normal sync and ask a different verified server for the subtree-root shortcut. A server that does not provide roots no longer makes every rescan wait through the same timeout.

**Technical detail:** Root support is cached per endpoint and shielded pool. During a historical Auto sync, canonical endpoint validation starts alongside local setup. Root requests prefer previously capable pool members and retry only through channels whose chain metadata and common block anchor were validated. Discovery and RPC timeouts are bounded independently, so pool validation can finish without extending the request budget or blocking the scan. Each candidate retains the selected transport and its own TLS identity. A usable root can skip rebuilding up to 65,536 historical commitments, but an unavailable optimization cannot block correctness or normal leaf replay.

### Atomic Dual-Pool Checkpoints

**In plain terms:** Sapling and Ironwood are saved at one shared chain point, so a crash cannot leave one pool ahead of the other.

**Technical detail:** Both trees receive their retained checkpoint and cursor updates in the same ordered transaction. Mini, major, replay, and emergency retention anchors preserve useful rollback points without keeping every historical state forever.

## Persistence and ownership

### Single-Owner Persistence Worker

**In plain terms:** Many cores can prepare work, but only one worker is allowed to write wallet and tree state, in order.

**Technical detail:** Scanners transfer immutable owned batches through a bounded channel. One persistence worker owns SQLite writer access, both ShardTrees, the sparse shard cache, and commit ordering. This removes writer races and nested database acquisition while still allowing download, decryption, and tree-fragment construction to overlap.

### Ownership Transfer Instead of Cloning

**In plain terms:** Large batches are handed to the writer instead of copied on the way there.

**Technical detail:** Commitment and discovery batches move into the persistence job by ownership. Parallel workers return owned fragments. Avoiding full immutable-batch clones reduces allocations, memory bandwidth, and peak memory without introducing shared mutation.

### One Atomic Wallet-State Commit

**In plain terms:** A batch is either completely visible or not visible at all.

**Technical detail:** Notes, nullifiers, spends, transaction summaries, tree deltas, checkpoints, repair state, the sync cursor, and the rolling canonical hash window are committed together. Progress is published only after success. A failed transaction invalidates speculative in-memory state before the batch can be retried.

### Explicit Database Contexts

**In plain terms:** Code that already has the database does not quietly open it again or fight itself for the writer lock.

**Technical detail:** Hot paths receive explicit read or write contexts. Reads use stable snapshots and do not perform hidden migrations. Writes serialize through the owner with a bounded busy timeout. The rebuildable compact-block cache uses SQLite WAL mode and appropriately relaxed durability, while authoritative wallet state keeps its stronger transaction boundary.

## Device adaptation and pipeline control

### Coarse Device Profiles

**In plain terms:** A phone and a workstation should not receive the same workload, but the server should not learn a precise hardware fingerprint from request sizes.

**Technical detail:** CPU, memory, and queue budgets are selected from coarse capability classes. Exact local scan sizes then adapt behind the continuous stream. A crash downgrade reduces the next session's pressure, and successful verified tips allow gradual recovery toward the normal profile.

### Saturation-Seeking Batch Controller

**In plain terms:** The controller looks for the point where more work stops increasing useful throughput, rather than chasing an arbitrary target time per batch.

**Technical detail:** Cached and live-network observations are separated. The local controller tracks blocks per second, active worker saturation, queue pressure, encoded bytes, and memory high-water marks. It grows while throughput improves, holds near the measured plateau, and backs off on pressure or regression. Hard byte and memory ceilings remain authoritative.

Network-fed ranges also use an adaptive shielded-work target so a dense range cannot turn into one long unresponsive step. Blocks already durable in the local cache use a larger per-worker work ceiling to amortize trial-decryption and frontier setup. Cached work remains bounded by that ceiling, exact encoded bytes, and the device profile's block limit, so unusually dense cached ranges still split without fragmenting ordinary rescans.

### Parallel Rescan Setup

**In plain terms:** Independent setup requests happen together, so the wallet can begin useful work sooner.

**Technical detail:** Tip discovery, chain-spec checks, cache-horizon validation, and optional subtree-root discovery are scheduled according to their dependencies instead of serialized by convenience. Only mandatory state gates the first scan batch.

## Correctness and recovery

Performance work in a shielded wallet is acceptable only when the optimized path produces the same usable wallet state as the reference path. Supernova treats servers, caches, background tasks, and speculative work as fallible inputs around one ordered state machine.

### Consensus and Canonical-Boundary Validation

**In plain terms:** The wallet checks that the server is on the expected Pirate Chain rules and that new blocks really continue from the state it already saved.

**Technical detail:** Consensus branch identifiers are compared as opaque protocol values at the target height. One tip snapshot bounds each run. Every range validates height order, block hashes, parent-hash continuity, transaction-hash size, Sapling nullifier and output shapes, Ironwood action shapes, and the persisted-to-incoming boundary before decryption results or subtree roots can be committed.

### Cache Integrity and Recovery

**In plain terms:** The wallet can prove that a cached block has not changed since it was accepted. If local cache bytes are damaged, it downloads that range again instead of scanning questionable data.

**Technical detail:** Version-two cache records checksum the exact protobuf payload, while range validation checks their structure, requested heights, chain links, and remote canonical end anchor. Validation failure invalidates the affected cache horizon and falls back to the normal network path. Legacy records receive the same canonical and structural checks before being rewritten into the checksummed format.

### Common-Checkpoint Reorg Recovery

**In plain terms:** If the chain changes, both shielded pools rewind to the same last-known-good point and replay from there.

**Technical detail:** The rolling canonical hash window locates a bounded common ancestor. Sapling and Ironwood roll back to a shared retained checkpoint, wallet state above it is reverted transactionally, and compact-block and sparse-tree caches above the fork are invalidated. Normal birthday sync still begins at the wallet birthday; replay below it occurs only when recovery requires older tree state.

### Interruption-Safe Task Ownership

**In plain terms:** Closing, cancelling, switching wallets, or pressing rescan twice cannot leave a hidden writer running or start two syncs over the same wallet.

**Technical detail:** A per-wallet operation lock gives each sync one task owner. Destructive operations cancel and join the active task first. Timed-out tasks remain registered until they actually finish, completion is idempotent, and speculative lookahead is dropped before rollback or restart.

### Stable-Snapshot Witness Repair

**In plain terms:** If a spend witness is missing or stale, the wallet records a repair job and rebuilds it from a known checkpoint instead of guessing.

**Technical detail:** Witness checks run against a stable database snapshot. Repairs are activated atomically, rewind to a retained checkpoint, replay the required range through the normal tree rules, and verify final witnesses at tip. A failed repair remains explicit rather than silently presenting an unspendable note as healthy.

### Semantic Differential Oracle

**In plain terms:** The same compact blocks are run through the reference and optimized pipelines, then the resulting wallets are compared field by field. A faster result is rejected if anything meaningful differs.

**Technical detail:** The oracle compares eight domains:

1. Balances
2. Notes and their deterministic serialized fields
3. Transaction history
4. Nullifiers and spend state
5. Sapling and Ironwood roots and checkpoints
6. Witnesses and marked positions
7. Repair queues
8. Sync cursors and the canonical hash window

Deterministic values and blobs are compared exactly, including their bytes. Randomized at-rest ciphertext and non-semantic creation timestamps are normalized and compared by their decrypted meaning. For that reason, the guarantee is stronger and more useful than comparing two SQLite files: Supernova must produce the same wallet state as the reference path, but it is not required to reproduce random encryption nonces or page layout.

The differential cases include a clean baseline, interruption and resume, rollback and reorg, sparse-cache reuse, cache invalidation, worker cancellation, witness state, and checkpoint preservation. The long-lived sparse-cache benchmark also refuses to report its speed result until the semantic oracle passes.

### Test Inventory

At the time this document was updated, the two core sync and persistence layers contained **505 Rust test functions**:

| Area | Test functions |
| --- | ---: |
| Sync source unit and async tests | 235 |
| Sync integration and benchmark tests | 54 |
| SQLite storage source tests | 124 |
| Storage integration and migration tests | 92 |
| **Total** | **505** |

Of those, **466 are not marked ignored** and **39 are explicit manual benchmarks or live-network scenarios**. Six active scenarios directly exercise or validate the semantic differential oracle, and one manual performance benchmark is guarded by the same oracle. The suite also includes seven opt-in live interruption/recovery scenarios and eight dedicated architecture guards for witness repair.

The raw count is not the guarantee by itself. The important part is the shape of the suite: optimized and reference paths consume identical immutable inputs; failure is injected between stages; and the comparison includes trees, witnesses, repairs, and cursors rather than stopping at the displayed balance.

## Observability and user-facing behavior

### Full-Stage Telemetry

**In plain terms:** When sync slows down, the logs show which stage consumed the time instead of leaving us to infer it from a progress bar.

**Technical detail:** Timings and counters cover fetch waits, cache reads, validation, trial decryption, commitment preparation, per-pool hashing and insertion, preload discovery, shard reads, cache hits/misses/evictions, checkpoint processing, dirty shard counts and encoded bytes, flushes, writer-lock wait, SQLite commits, queue pressure, and memory high-water marks. Sapling and Ironwood tree metrics are separated.

This instrumentation changed the optimization strategy several times. For example, one earlier profile showed 22.94 seconds in tree/frontier work, 14.84 seconds of trial-decryption CPU almost completely hidden by overlap, 170 milliseconds persisting notes, 72 milliseconds committing SQLite, and 21 milliseconds flushing shards. That made it clear that another database shortcut would not recover seconds; tree construction was the next target.

### Honest Progress and Live Throughput

**In plain terms:** The app names setup work, reports speed from the latest completed batch, and does not claim a block is synced before it is committed.

**Technical detail:** Preparing, tree-state retrieval, fetching, scanning, persistence, repair, and tip verification are distinct stages. The blocks-per-second display uses recent completed work rather than a whole-session mean, while ETA can remain smoothed. State polling is observational and cannot start, cancel, or mutate a sync as a side effect.

## Measured results

These figures come from development profiling on a warm local compact-block cache. They are useful for tracking regressions on the same corpus and machine; they are not a promise that every device or live server will produce the same number.

| Measurement | Observed result | Scope |
| --- | ---: | --- |
| Protobuf versus legacy JSON cache decoding | about **70x faster** | Codec/cache microbenchmark |
| Version-two cache integrity envelope | **+3.87%**, about **5 ms per 65,536 blocks** | Optimized decode-only microbenchmark |
| Representative cached rescan | about **583,000 blocks in the low-20-second range** | Complete local rescan on the development Windows machine |
| Representative end-to-end cached throughput | roughly **mid-20,000s blocks/s** | Includes scanning, trees, persistence, and verification |
| Fast completed batches | **over 50,000 blocks/s** | Short batch peak, not whole-run throughput |
| Lookahead decryption wait in one profile | **31 ms** from **14.84 s** of decrypt CPU | Demonstrates pipeline overlap |

Live first-time sync is additionally bounded by endpoint throughput, latency, selected privacy transport, and the density of shielded actions in the requested range. Blocks per second alone can also be misleading: an empty compact block and a block containing many shielded actions have very different costs. Repeatable comparisons should report block range, encoded bytes, shielded actions, cache state, hardware, and transport.

## Design rules that made the speed safe

- Parallelize immutable preparation; serialize canonical mutation.
- Bound queues by actual bytes and memory, not block-count guesses alone.
- Treat caches, speculative work, and server shortcuts as disposable accelerators.
- Validate order and boundaries before accepting expensive downstream work.
- Commit wallet state, trees, checkpoints, and cursors as one visible transition.
- Preserve a deterministic reference path and compare the complete semantic result.
- Measure stage time before optimizing; the bottleneck moves after every large win.
- Keep device adaptation local so transport privacy does not depend on exact hardware timing.

None of the headline gains came from skipping trial decryption, relaxing chain validation, replacing ShardTree with an easier structure, trusting lightwalletd with viewing keys, or allowing concurrent database writers. Supernova became fast by making the necessary work overlap cleanly and by removing work that never needed to exist.

## Implementation map

- Compact-block cache: [`pirate-sync-lightd/src/block_cache.rs`](../crates/pirate-sync-lightd/src/block_cache.rs)
- Ordered stream validation: [`pirate-sync-lightd/src/ordered_stream.rs`](../crates/pirate-sync-lightd/src/ordered_stream.rs)
- Intake and adaptive buffering: [`pirate-sync-lightd/src/intake.rs`](../crates/pirate-sync-lightd/src/intake.rs)
- Sync pipeline and ShardTree worker: [`pirate-sync-lightd/src/sync.rs`](../crates/pirate-sync-lightd/src/sync.rs)
- Device profiles: [`pirate-sync-lightd/src/sync_profile.rs`](../crates/pirate-sync-lightd/src/sync_profile.rs)
- SQLite wallet state: [`pirate-storage-sqlite`](../crates/pirate-storage-sqlite/)
- Performance and recovery tests: [`pirate-sync-lightd/tests`](../crates/pirate-sync-lightd/tests/)
