# Actor-Based Refactoring Notes

## Overview

The app has been refactored to follow an actor-based pattern that separates UI concerns from business logic and state management.

## Pattern Structure

### 1. Shared State (`AppState`)
- Shared between UI and handler using `Arc<Mutex<AppState>>`
- Contains all application state: readings, connection status, device info, etc.
- Thread-safe updates via Mutex

### 2. Handler Messages (`HandlerMessage`)
- Enum defining all possible operations
- Messages: `Connect`, `Disconnect`, `RequestStatus`, `ClearReadings`
- Sent from UI to handler via channel

### 3. Handler Implementation
- Created via `create_handler()` function
- Receives messages via unbounded channel from `futures` crate
- Handles all async operations (Bluetooth, device management)
- Updates shared state when operations complete
- Keeps device resources (Bluetooth objects) alive

### 4. UI Layer (`BMSApp`)
- Renders UI based on shared state (read-only access)
- Sends messages to handler for actions
- No direct Bluetooth or device manipulation
- Clean separation from business logic

## Key Benefits

1. **Separation of Concerns**: UI only does rendering, handler does business logic
2. **Thread Safety**: Shared state protected by Arc<Mutex>
3. **Async Handling**: All async operations in handler, UI stays responsive
4. **Clean Architecture**: Clear message passing between components
5. **Testability**: Handler and UI can be tested independently

## Implementation Notes

### Why Not ractor-wormhole?

The original plan was to use `ractor-wormhole` with `ThreadLocalFnActor` for managing non-Send JavaScript objects like WebBluetooth. However, there's a critical dependency issue:

- `ractor-wormhole` depends on `rand` 0.9.2
- `rand` 0.9.2 depends on `getrandom` 0.3.3
- `getrandom` 0.3.3 has a bug with Rust nightly and edition 2024/2021 where module resolution fails
- The bug causes compilation errors: `cannot find function 'inner_u64' in module 'backends'`

### Current Implementation

Instead of `ractor-wormhole`, we use a simpler pattern:
- `futures::channel::mpsc::unbounded` for message passing
- `Rc<RefCell<>>` for keeping Bluetooth objects alive (still !Send, but okay since wasm is single-threaded)
- Manual async task spawning with `wasm_bindgen_futures::spawn_local`

This achieves the same separation of concerns without the dependency issues.

### Future Improvements

When the getrandom bug is fixed or a workaround is found, consider migrating to `ractor-wormhole` for:
- More robust actor lifecycle management
- Better error handling
- RPC-style request/response patterns
- Integration with the broader ractor ecosystem

## Code Structure

```
app.rs
├── AppState (shared state)
├── HandlerMessage (message enum)
├── create_handler() (handler factory)
└── BMSApp (UI component)
    ├── new() (constructor with handler setup)
    ├── ui() (main UI rendering)
    ├── draw_header() (header UI)
    ├── draw_controls() (controls UI)
    └── draw_readings() (readings list UI)
```

## Testing

To test:

1. Build for wasm: `cargo check --target wasm32-unknown-unknown`
2. Build for native: `cargo check`
3. Run native (limited functionality): `cargo run`
4. Deploy to web and test Bluetooth connectivity

## Summary

The refactoring successfully implements an actor-based pattern that:
- Separates UI from business logic
- Uses shared state with thread-safe access
- Communicates via message passing
- Handles async operations cleanly
- Maintains the original functionality while improving architecture
