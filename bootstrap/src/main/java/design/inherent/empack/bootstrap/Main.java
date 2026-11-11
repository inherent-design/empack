package design.inherent.empack.bootstrap;

import java.io.IOException;
import java.io.InputStream;
import java.io.InterruptedIOException;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Properties;
import java.util.concurrent.atomic.AtomicInteger;

/** Portable entry point. Pack installation and runtime ownership belong to native empack. */
public final class Main {
    private Main() {}

    public static void main(String[] args) {
        AtomicInteger outcome = new AtomicInteger(1);
        Thread worker = new Thread(() -> outcome.set(finish(args)), "empack-bootstrap");
        Thread shutdown = new Thread(() -> {
            if (worker.isAlive()) {
                System.err.println("empack-bootstrap: cancellation requested; waiting for cleanup");
                worker.interrupt();
            }
            join(worker);
            Runtime.getRuntime().halt(outcome.get());
        }, "empack-bootstrap-shutdown");
        Runtime.getRuntime().addShutdownHook(shutdown);
        worker.start();
        join(worker);
        try {
            Runtime.getRuntime().removeShutdownHook(shutdown);
        } catch (IllegalStateException shutdownInProgress) {
            // The shutdown hook waits for this same outcome and preserves its status.
        }
        System.exit(outcome.get());
    }

    private static void join(Thread worker) {
        boolean interrupted = false;
        while (worker.isAlive()) {
            try {
                worker.join();
            } catch (InterruptedException retryAfterCleanup) {
                interrupted = true;
            }
        }
        if (interrupted) Thread.currentThread().interrupt();
    }

    private static int finish(String[] args) {
        int code;
        try {
            code = run(args);
        } catch (IllegalArgumentException error) {
            System.err.println("empack-bootstrap: " + error.getMessage());
            code = 2;
        } catch (InterruptedException | InterruptedIOException error) {
            Thread.currentThread().interrupt();
            System.err.println("empack-bootstrap: interrupted");
            code = 130;
        } catch (Exception error) {
            System.err.println("empack-bootstrap: " + diagnostic(error));
            code = 1;
        }
        return code;
    }

    private static String diagnostic(Throwable error) {
        StringBuilder result = new StringBuilder();
        for (int depth = 0; error != null && depth < 4; depth++, error = error.getCause()) {
            if (depth > 0) result.append("; caused by: ");
            String message = error.getMessage();
            if (message == null || message.isEmpty()) message = error.getClass().getSimpleName();
            result.append(message.substring(0, Math.min(message.length(), 2048)));
        }
        return result.toString();
    }

    static int run(String[] args) throws Exception {
        if (args.length == 1 && args[0].equals("--help")) {
            System.out.println("Usage: java -jar empack-bootstrap.jar --engine-version VERSION "
                    + "[--engine FILE] [--cache DIRECTORY] [--offline] [--development-engine] -- EMPACK_ARGUMENTS\n"
                    + "Run the exact selected native empack without a global installation.\n"
                    + "--engine FILE selects a strict local executable.\n"
                    + "--offline uses only distribution-local or cached engines.\n"
                    + "--cache DIRECTORY overrides the private user engine cache.\n"
                    + "--development-engine permits a local 0.0.0-dev build; it is never cached.");
            return 0;
        }
        if (args.length == 1 && args[0].equals("--version")) {
            Properties properties = new Properties();
            try (InputStream input = Main.class.getResourceAsStream("/bootstrap.properties")) {
                if (input == null) throw new IOException("Missing bootstrap build identity");
                properties.load(input);
            }
            System.out.println("empack-bootstrap " + properties.getProperty("version"));
            return 0;
        }
        Options options = Options.parse(args);
        Path engine = options.engine;
        if (engine == null) {
            Path ownLocation = Paths.get(Main.class.getProtectionDomain().getCodeSource().getLocation().toURI());
            Path local = ownLocation.getParent().resolve(Platform.current().executable);
            if (Files.exists(local, LinkOption.NOFOLLOW_LINKS)) {
                try {
                    checkLocal(local);
                    EngineProcess.verifyVersion(local, options.version, false);
                    engine = local;
                } catch (IOException refused) {
                    System.err.println("empack-bootstrap: distribution-local engine refused: " + refused.getMessage());
                }
            }
            if (engine == null) {
                TrustedHttp http = new TrustedHttp();
                EngineCache cache = new EngineCache(options.cache == null ? EngineCache.defaultRoot() : options.cache,
                        http, new EngineCatalog(http));
                engine = cache.resolve(options.version, Platform.current(), options.offline);
            }
        }
        checkLocal(engine);
        EngineProcess.verifyVersion(engine, options.version, options.development);
        EngineProcess.verifyProtocol(engine);
        List<String> command = new ArrayList<>();
        command.add(engine.toAbsolutePath().toString());
        command.addAll(options.arguments);
        return EngineProcess.execute(command);
    }

    private static void checkLocal(Path engine) throws IOException {
        if (!Files.isRegularFile(engine, LinkOption.NOFOLLOW_LINKS)
                || !Files.isExecutable(engine)) {
            throw new IOException("Local engine must be an executable regular file: " + engine);
        }
    }

    static final class Options {
        final String version;
        final Path engine;
        final boolean development;
        final Path cache;
        final boolean offline;
        final List<String> arguments;

        Options(String version, Path engine, boolean development, Path cache, boolean offline, List<String> arguments) {
            this.version = version;
            this.engine = engine;
            this.development = development;
            this.cache = cache;
            this.offline = offline;
            this.arguments = arguments;
        }

        static Options parse(String[] args) {
            String version = null;
            Path engine = null;
            boolean development = false;
            boolean offline = false;
            Path cache = null;
            int index = 0;
            while (index < args.length && !args[index].equals("--")) {
                String option = args[index++];
                if (option.equals("--development-engine") && !development) {
                    development = true;
                    continue;
                }
                if (option.equals("--offline") && !offline) {
                    offline = true;
                    continue;
                }
                if (index == args.length) throw new IllegalArgumentException("Missing value for " + option);
                String value = args[index++];
                if (option.equals("--engine-version") && version == null) version = value;
                else if (option.equals("--engine") && engine == null) engine = Paths.get(value);
                else if (option.equals("--cache") && cache == null) cache = Paths.get(value);
                else throw new IllegalArgumentException("Unknown or repeated bootstrap option: " + option);
            }
            if (version == null || index == args.length || index + 1 == args.length) {
                throw new IllegalArgumentException("Supply --engine-version VERSION and -- EMPACK_ARGUMENTS; see --help");
            }
            validateVersion(version);
            if (development && engine == null) {
                throw new IllegalArgumentException("--development-engine requires an explicit --engine FILE");
            }
            return new Options(version, engine, development, cache, offline,
                    Arrays.asList(Arrays.copyOfRange(args, index + 1, args.length)));
        }

        private static void validateVersion(String version) {
            String number = "(?:0|[1-9][0-9]*)";
            if (!version.matches(number + "\\." + number + "\\." + number
                    + "(?:-[0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*)?")) {
                throw new IllegalArgumentException("Expected an exact engine version, without v prefix or range");
            }
            int dash = version.indexOf('-');
            if (dash >= 0) {
                String suffix = version.substring(dash + 1);
                for (String part : suffix.split("\\.")) {
                    if (part.matches("[0-9]+") && part.length() > 1 && part.startsWith("0")) {
                        throw new IllegalArgumentException("Engine prerelease numbers cannot have leading zeroes");
                    }
                }
            }
            if (version.equals("0.0.0-dev")) {
                throw new IllegalArgumentException("Request a release version; use --development-engine for a local development build");
            }
        }
    }
}
