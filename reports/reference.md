# Empack Architecture Reference Guide

## 1. System Overview

**Core Principle:** Compositional Orchestration

`empack` is designed as a compositional orchestrator, where each component is a self-contained unit of functionality that can be combined with other components to create complex workflows. This approach promotes modularity, reusability, and maintainability.

**Key Architectural Components:**
- **`empack` (Binary):** The main entry point of the application, responsible for initializing the `empack-lib` and executing commands.
- **`empack-lib` (Library):** The core of the application, containing all the business logic, modules, and primitives.
- **Modules:** Self-contained units of functionality, such as `networking`, `platform`, and `terminal`.
- **Primitives:** The foundational data types, enums, and error-handling strategies that are shared across all modules.

## 2. Module Relationships

The modules in `empack-lib` are designed to be loosely coupled and highly cohesive.

- **`application`:** The top-level module that orchestrates the other modules to execute commands. It depends on all other modules.
- **`networking`:** Provides asynchronous HTTP client functionality and is used by the `empack` module to download modpacks and other resources.
- **`platform`:** Detects system resources and provides platform-specific optimizations. It is used by the `networking` module to configure the HTTP client.
- **`terminal`:** Provides cross-platform terminal capability detection and is used by the `logger` and `display` modules.
- **`logger`:** Provides structured logging with progress tracking and is used by all other modules.
- **`display`:** Provides a rich display system with progress bars and other UI elements.
- **`primitives`:** The foundational module that is used by all other modules.

## 3. Design Patterns

`empack` leverages several design patterns to achieve its modular and maintainable architecture.

- **Composition over Inheritance:** The application favors composition over inheritance, as seen in the `AppConfig` struct, which is composed of smaller configuration structs.
- **Dependency Injection:** The `main` function in `empack-lib/src/lib.rs` injects the `AppConfig` and `TerminalCapabilities` into the `Logger` and `Display` modules, which is a form of dependency injection.
- **State Machine:** The `ModpackState` enum and `StateTransition` enum in `empack-lib/src/primitives/empack.rs` implement a state machine pattern to manage the modpack development lifecycle.

## 4. Actionable Recommendations

- **Formalize Dependency Injection:** While dependency injection is used in some places, it could be applied more consistently throughout the application. For example, the `NetworkingManager` could be injected into the `empack` module instead of being created directly.
- **Document Architectural Decisions:** Create a dedicated `architecture.md` file in the `docs` directory to document the key architectural decisions and design patterns used in the project. This will help new developers understand the codebase more quickly.
- **Explore a More Formal Orchestration Framework:** While the current orchestration logic in the `application` module is effective, a more formal framework like a command bus or an event-driven architecture could provide more flexibility and scalability in the long run.
