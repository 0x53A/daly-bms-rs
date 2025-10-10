# Architecture Comparison

## Before Refactoring

```
┌──────────────────────────────────────┐
│           BMSApp (UI)                │
│                                      │
│  ┌────────────────────────────────┐ │
│  │ Rc<RefCell<Vec<Reading>>>      │ │
│  │ Rc<RefCell<Vec<Closures>>>     │ │
│  │ Rc<RefCell<Option<Device>>>    │ │
│  │ Rc<RefCell<Option<ControlChar>>│ │
│  └────────────────────────────────┘ │
│                                      │
│  • UI code directly spawns async    │
│    tasks for Bluetooth operations   │
│  • Direct device manipulation       │
│  • State scattered across RefCells  │
│  • Tightly coupled UI and logic     │
│                                      │
│  On button click:                   │
│  └─> spawn_local(bt::open_device()) │
│       └─> spawn_local(rx loop)      │
│            └─> readings.push()      │
└──────────────────────────────────────┘
```

## After Refactoring

```
┌─────────────────────────────────────────────────────────┐
│                     Application                         │
│                                                         │
│  ┌────────────────────┐      ┌────────────────────┐   │
│  │   BMSApp (UI)      │      │   Handler          │   │
│  │                    │      │   (Async Actor)    │   │
│  │  • Renders state   │      │                    │   │
│  │  • Sends messages  │      │  • Manages device  │   │
│  │  • Read-only       │      │  • Processes msgs  │   │
│  │    state access    │      │  • Updates state   │   │
│  └────────┬───────────┘      └──────┬─────────────┘   │
│           │                          │                  │
│           │   Messages via Channel   │                  │
│           │  (Connect, Disconnect,   │                  │
│           │   RequestStatus, etc.)   │                  │
│           └────────────┬─────────────┘                  │
│                        │                                │
│              ┌─────────▼──────────┐                     │
│              │  Arc<Mutex<State>> │                     │
│              │                    │                     │
│              │  • readings        │                     │
│              │  • is_connected    │                     │
│              │  • device_name     │                     │
│              │  • last_update     │                     │
│              │  • status_message  │                     │
│              └────────────────────┘                     │
│                                                         │
│  Flow:                                                  │
│  User clicks button                                     │
│   └─> UI sends HandlerMessage::Connect                 │
│        └─> Handler receives message                     │
│             └─> Handler calls bt::open_device()         │
│                  └─> Handler updates Arc<Mutex<State>> │
│                       └─> UI reads updated state        │
│                            └─> UI re-renders            │
└─────────────────────────────────────────────────────────┘
```

## Key Differences

### State Management
**Before:** `Rc<RefCell<T>>` - single-threaded, scattered state
**After:** `Arc<Mutex<AppState>>` - thread-safe, centralized state

### Communication
**Before:** Direct function calls, shared RefCells
**After:** Message passing via channels

### Separation
**Before:** UI and business logic mixed
**After:** Clear separation - UI renders, Handler acts

### Async Handling
**Before:** spawn_local called directly from UI
**After:** All async in Handler, UI stays synchronous

### Testability
**Before:** Difficult - UI and logic tightly coupled
**After:** Easy - Handler and UI can be tested independently

### Code Organization
**Before:** All in one impl block, hard to follow
**After:** Clear sections: State, Messages, Handler, UI

## Message Flow Example

```
User clicks "Connect" button
   │
   ├─> UI: button clicked
   │
   ├─> UI: handler.send(HandlerMessage::Connect)
   │
   ├─> Handler: receives Connect message
   │
   ├─> Handler: spawn async bt::open_device()
   │
   ├─> Handler: device connects successfully
   │
   ├─> Handler: lock state, update fields
   │       state.is_connected = true
   │       state.device_name = Some(name)
   │       state.status_message = "Connected..."
   │
   ├─> Handler: spawn rx loop for readings
   │
   ├─> UI: on next frame, lock state
   │
   ├─> UI: read state fields
   │
   └─> UI: render updated UI with new state
```

## Benefits Achieved

✅ **Single Responsibility**: UI only renders, Handler only processes
✅ **Encapsulation**: State changes only through Handler
✅ **Thread Safety**: Arc<Mutex> ensures safe concurrent access
✅ **Scalability**: Easy to add new messages and handlers
✅ **Debugging**: Clear message trail, easier to trace issues
✅ **Testing**: Mock handler or state for unit tests
✅ **Maintainability**: Changes to UI or Handler independent
