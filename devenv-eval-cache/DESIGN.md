# Eval Cache Design

Transparent caching layer for FFI-based Nix evaluation in devenv.

## Overview

The eval cache stores JSON results from `NixRustBackend.eval()` calls. On subsequent evaluations with the same configuration and unchanged inputs, cached results are returned without invoking Nix.

## Cache Key Design

```
cache_key = blake3(serialize(NixArgs) + ":" + attr_name)
```

### Components

| Component | Source | Purpose |
|-----------|--------|---------|
| `NixArgs` | Serialized via `ser_nix` | All evaluation configuration |
| `attr_name` | e.g., `"config.shell"` | Which attribute to evaluate |

### What NixArgs Captures

- `version` - CLI version (behavior changes)
- `system` - Target architecture
- `devenv_root` - Project location
- `active_profiles` - Enabled profiles
- `container_name` - Container context
- `devenv_config` - Full devenv.yaml
- `nixpkgs_config` - Nixpkgs settings

### Why import_expr Is Not in the Key

The bootstrap expression (`import_expr`) is tracked via observed file inputs during evaluation. When any file it imports changes, the cache is invalidated. This avoids duplicating the tracking.

## Input Validation

Cache hits are validated by checking that observed inputs haven't changed:

```
+------------------+     +-------------------+
|   Cache Key      |     |  Observed Inputs  |
| (before eval)    |     |  (during eval)    |
+------------------+     +-------------------+
| NixArgs hash     |     | Files read        |
| + attr_name      |     | Env vars accessed |
+------------------+     +-------------------+
        |                        |
        v                        v
   Lookup in DB            Validate state
```

### File Input Validation

For each file observed during the original evaluation:
1. Check if file still exists
2. Compare content hash
3. Compare modification time

### Environment Variable Validation

For each env var accessed:
1. Check current value
2. Compare content hash

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    NixRustBackend                       │
│  ┌───────────────────────────────────────────────────┐  │
│  │              CachingEvalState<E>                  │  │
│  │  ┌─────────────┐  ┌────────────┐  ┌────────────┐  │  │
│  │  │ eval_state  │  │ CachedEval │  │nix_args_str│  │  │
│  │  │   (E)       │  │            │  │            │  │  │
│  │  └─────────────┘  └─────┬──────┘  └────────────┘  │  │
│  └─────────────────────────┼─────────────────────────┘  │
│                            │                            │
│  ┌─────────────────────────┼─────────────────────────┐  │
│  │           CachingEvalService                      │  │
│  │                         │                         │  │
│  │  ┌──────────────────────┴──────────────────────┐  │  │
│  │  │                 SQLite DB                   │  │  │
│  │  │  ┌────────────┐  ┌──────────┐  ┌─────────┐  │  │  │
│  │  │  │cached_eval │  │file_input│  │env_input│  │  │  │
│  │  │  └────────────┘  └──────────┘  └─────────┘  │  │  │
│  │  └─────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### CachingEvalState<E>

Wrapper that enforces caching for all evaluation operations:

```rust
pub struct CachingEvalState<E> {
    eval_state: E,           // Private - no direct access
    cached_eval: CachedEval,
    nix_args_str: String,    // Pre-serialized for key generation
}
```

**Methods:**
- `cache_key(attr_name)` - Generate key for an attribute
- `cached_eval()` - Access the caching service
- `uncached(reason)` - Explicit bypass with justification
- `into_inner()` - Consume wrapper, get eval_state

### CachedEval

Transparent caching interface:

```rust
// With caching
let cached_eval = CachedEval::with_cache(service, log_bridge, config);

// Without caching (passthrough)
let cached_eval = CachedEval::without_cache(log_bridge);

// Same interface either way
let (result, cache_hit) = cached_eval.eval(&key, || async {
    // Actual evaluation
    Ok(json_string)
}).await?;

// Or with automatic JSON serialization/deserialization
let (typed_result, cache_hit) = cached_eval.eval_typed::<MyType, _, _>(&key, || async {
    Ok(my_typed_value)
}).await?;
```

### UncachedReason

Documents legitimate cache bypass cases:

```rust
pub enum UncachedReason {
    LockValidation,  // Must check fresh state
    Repl,            // Interactive, no caching value
    Update,          // Modifies state
    Search,          // Large/dynamic results
}
```

## Database Schema

### cached_eval

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER | Primary key |
| key_hash | TEXT | blake3(NixArgs + attr_name) |
| attr_name | TEXT | Human-readable attribute |
| input_hash | TEXT | Hash of all input hashes |
| json_output | TEXT | Cached JSON result |
| updated_at | INTEGER | Last access time |

### file_input

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER | Primary key |
| path | BLOB | File path (bytes) |
| is_directory | BOOLEAN | Directory flag |
| content_hash | TEXT | blake3 of content |
| modified_at | INTEGER | mtime at cache time |
| updated_at | INTEGER | Last check time |

### eval_input_path

| Column | Type | Description |
|--------|------|-------------|
| cached_eval_id | INTEGER | FK to cached_eval |
| file_input_id | INTEGER | FK to file_input |

### eval_env_input

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER | Primary key |
| cached_eval_id | INTEGER | FK to cached_eval |
| name | TEXT | Env var name |
| content_hash | TEXT | Hash of value |
| updated_at | INTEGER | Last check time |

## Input Collection

During evaluation, `EvalInputCollector` observes operations via `NixLogBridge`:

```rust
let collector = EvalInputCollector::start();
log_bridge.add_observer(collector.clone());

// ... evaluation runs, collector receives EvalOp events ...

log_bridge.clear_observers();
let inputs = collector.into_inputs(&config);
```

### Observed Operations

| EvalOp | Tracked As |
|--------|------------|
| ReadFile | File input |
| ReadDir | File input (directory) |
| PathExists | File input |
| EvaluatedFile | File input |
| TrackedPath | File input |
| CopiedSource | File input |
| GetEnv | Env input |

### Filtering

Inputs are filtered before storage:
- Skip `/nix/store/*` (immutable)
- Skip non-absolute paths
- Skip paths in `config.excluded_paths`
- Add paths from `config.extra_watch_paths`

## Usage Flow

### Cache Miss

```
1. CachingEvalState.cache_key("config.shell")
2. CachedEval.eval(key, || eval_fn())
   a. Check DB for key_hash → miss
   b. Start EvalInputCollector
   c. Run eval_fn() → JSON result
   d. Collect observed inputs
   e. Store (key_hash, inputs, result) in DB
3. Return (result, cache_hit=false)
```

### Cache Hit

```
1. CachingEvalState.cache_key("config.shell")
2. CachedEval.eval(key, || eval_fn())
   a. Check DB for key_hash → found
   b. Load file_input and env_input rows
   c. Validate each input still matches
   d. All valid → return cached JSON
3. Return (result, cache_hit=true)
```

### Invalidation

Cache entries are invalidated when:
- Any observed file is modified/removed
- Any observed env var changes
- `config.force_refresh = true`

No TTL - entries are valid until inputs change.

## Graceful Degradation

Cache failures don't block evaluation:

```rust
match service.get_cached(key).await {
    Ok(Some(cached)) => return Ok((cached.json_output, true)),
    Ok(None) => { /* cache miss - evaluate */ }
    Err(e) => {
        warn!(error = %e, "Cache lookup failed, proceeding with evaluation");
        // Continue to evaluate
    }
}
```

## Configuration

```rust
pub struct CachingConfig {
    /// Force re-evaluation even if cache is valid
    pub force_refresh: bool,
    /// Additional paths to watch
    pub extra_watch_paths: Vec<PathBuf>,
    /// Paths to exclude from invalidation
    pub excluded_paths: Vec<PathBuf>,
}
```
