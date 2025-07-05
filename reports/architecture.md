# Empack System Architecture

## 1. Core Principles

**Compositional Orchestrator:** `empack` is architected as a compositional orchestrator, where independent, reusable components are combined to form complex, robust workflows. This design emphasizes modularity, clear interfaces, and a unidirectional data flow, ensuring that the system is both scalable and maintainable.

**Key Architectural Pillars:**
- **Separation of Concerns:** A strict separation between the `empack` binary (application entry point) and the `empack-lib` library (core logic) is maintained.
- **Layered Architecture:** The system is organized into distinct layers, with the `primitives` module at the core, followed by the `platform`, `networking`, and `terminal` modules, and finally the `application` module at the top.
- **Unidirectional Data Flow:** Data flows in a single direction, from the `application` module down to the `primitives` module, which ensures a predictable and easy-to-understand system.

## 2. System Components

### 2.1. `empack` (Binary)

The `empack` binary is a lightweight wrapper around the `empack-lib` library. Its sole responsibility is to:
1. Initialize the `tokio` runtime.
2. Call the `empack_lib::main()` function.
3. Handle any errors that propagate up from the library.

### 2.2. `empack-lib` (Library)

The `empack-lib` library contains the core logic of the application. It is organized into the following modules:

- **`primitives`:** Defines the foundational data types, enums, and error-handling strategies. This is the heart of the type system.
- **`platform`:** Detects system resources (CPU, memory) and provides platform-specific optimizations.
- **`networking`:** Provides a robust, asynchronous HTTP client with support for connection pooling, retries, and concurrency limiting.
- **`terminal`:** Detects terminal capabilities (color, Unicode, graphics) and provides a set of cross-platform terminal primitives.
- **`logger`:** Implements a structured, asynchronous logger with support for multiple output formats (text, JSON, YAML) and progress tracking.
- **`display`:** Provides a rich, interactive display system with progress bars, spinners, and other UI elements.
- **`application`:** The main orchestrator of the application. It parses command-line arguments, loads configuration, initializes the other modules, and executes the requested command.

## 3. Data Flow and Orchestration

The orchestration of the application is handled by the `application` module, which follows a clear and predictable sequence of operations:

1. **Configuration Loading:** The `AppConfig::load_with_command()` function loads configuration from multiple sources (defaults, `.env` file, environment variables, CLI arguments) and merges them into a single `AppConfig` struct.
2. **Terminal Capability Detection:** The `TerminalCapabilities::detect_from_config()` function detects the capabilities of the terminal and creates a `TerminalCapabilities` struct.
3. **Logger Initialization:** The `Logger::init()` function initializes the global logger with the `LoggerConfig` derived from the `AppConfig` and `TerminalCapabilities`.
4. **Display Initialization:** The `Display::init()` function initializes the display system with the `TerminalCapabilities`.
5. **Global Configuration:** The `AppConfig::init_global()` function stores the `AppConfig` in a global `OnceLock` for easy access from other modules.
6. **Command Execution:** The `execute_command()` function executes the requested command, using the other modules to perform the necessary operations.

## 4. Actionable Recommendations

- **Formalize the Orchestration Logic:** While the current orchestration logic is effective, it could be formalized into a more generic and reusable component. A dedicated `Orchestrator` struct could be created to manage the initialization and execution of the application, making the code more modular and easier to test.
- **Introduce a Service Locator or Dependency Injection Container:** As the application grows, a service locator or a dependency injection container could be used to manage the dependencies between modules. This would make the code more loosely coupled and easier to maintain.
- **Add Architectural Diagrams:** Create a set of architectural diagrams (e.g., using Mermaid.js) to visually represent the system architecture, data flow, and module relationships. This would make the architecture easier to understand for new developers.
