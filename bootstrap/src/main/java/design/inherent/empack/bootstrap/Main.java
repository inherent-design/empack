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
            System.err.println("empack-bootstrap: " + safeText(error.getMessage()));
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
        return safeText(result.toString());
    }

    static String safeText(String value) {
        StringBuilder result = new StringBuilder();
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            if ((Character.isISOControl(ch) && ch != '\n' && ch != '\t')
                    || ch == '\u061c' || ch == '\u200e' || ch == '\u200f'
                    || (ch >= '\u202a' && ch <= '\u202e') || (ch >= '\u2066' && ch <= '\u2069')) {
                result.append("\\u{").append(Integer.toHexString(ch)).append('}');
            } else {
                result.append(ch);
            }
        }
        return result.toString();
    }

    static int run(String[] args) throws Exception {
        if (args.length == 1 && args[0].equals("--help")) {
            System.out.println("Usage: java -Dfile.encoding=UTF-8 -jar empack-bootstrap.jar [OPTIONS] "
                    + "--engine-version VERSION -- EMPACK_ARGUMENTS\n"
                    + "   or: java -Dfile.encoding=UTF-8 -jar empack-bootstrap.jar [OPTIONS] "
                    + "--invocation-file FILE [-- RUNTIME_ARGUMENTS]\n"
                    + "Run the exact selected native empack without a global installation.\n"
                    + "--invocation-file .empack-tools/NAME.json supplies versioned UTF-8 static inputs;\n"
                    + "append runtime arguments only after -- when the record permits it.\n"
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
        return run(Options.parse(args));
    }

    static int run(Options options) throws Exception {
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
                    System.err.println("empack-bootstrap: distribution-local engine refused: " + safeText(refused.getMessage()));
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
        return EngineProcess.execute(command, options.directory);
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
        final Path directory;

        Options(String version, Path engine, boolean development, Path cache, boolean offline, List<String> arguments, Path directory) {
            this.version = version;
            this.engine = engine;
            this.development = development;
            this.cache = cache;
            this.offline = offline;
            this.arguments = arguments;
            this.directory = directory;
        }

        static Options parse(String[] args) throws IOException {
            String version = null;
            Path engine = null;
            Path invocation = null;
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
                else if (option.equals("--invocation-file") && invocation == null) invocation = Paths.get(value);
                else if (option.equals("--engine") && engine == null) engine = Paths.get(value);
                else if (option.equals("--cache") && cache == null) cache = Paths.get(value);
                else throw new IllegalArgumentException("Unknown or repeated bootstrap option: " + option);
            }
            Path directory = null;
            List<String> arguments = new ArrayList<>();
            if (invocation != null) {
                if (version != null) throw new IllegalArgumentException("Do not combine --engine-version with --invocation-file");
                InvocationFile record = InvocationFile.read(invocation);
                version = record.version;
                directory = record.directory;
                arguments.addAll(record.arguments);
                if (index < args.length) {
                    if (!record.appendArguments) throw new IllegalArgumentException("This invocation file does not accept runtime arguments");
                    arguments.addAll(Arrays.asList(Arrays.copyOfRange(args, index + 1, args.length)));
                }
                if (engine != null && !engine.isAbsolute()) engine = directory.resolve(engine).normalize();
            } else {
                if (version == null || index == args.length || index + 1 == args.length) {
                    throw new IllegalArgumentException("Supply --engine-version VERSION and -- EMPACK_ARGUMENTS, or --invocation-file FILE; see --help");
                }
                arguments.addAll(Arrays.asList(Arrays.copyOfRange(args, index + 1, args.length)));
            }
            validateVersion(version);
            if (development && engine == null) {
                throw new IllegalArgumentException("--development-engine requires an explicit --engine FILE");
            }
            return new Options(version, engine, development, cache, offline, arguments, directory);
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
