package design.inherent.empack.bootstrap;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.net.SocketException;
import java.net.SocketTimeoutException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.SecureRandom;
import java.util.List;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

final class EngineProcess {
    private EngineProcess() {}

    static void verifyVersion(Path engine, String expected, boolean development)
            throws IOException, InterruptedException {
        String actual = probe(engine, "--version");
        if (!(actual.equals("empack " + expected)
                || (development && actual.equals("empack 0.0.0-dev")))) {
            throw new IOException("Local engine does not report requested version " + expected
                    + "; observed " + actual);
        }
    }

    static void verifyProtocol(Path engine) throws IOException, InterruptedException {
        try {
            if (!probe(engine, "--bootstrap-protocol").equals("empack-bootstrap/1")) {
                throw new IOException("Unexpected bootstrap protocol response");
            }
        } catch (IOException unsupported) {
            throw new IOException("Selected engine lacks bootstrap cancellation protocol 1; "
                    + "use empack 0.8.5-beta or a newer compatible engine", unsupported);
        }
    }

    private static String probe(Path engine, String argument) throws IOException, InterruptedException {
        ProcessBuilder builder = new ProcessBuilder(engine.toAbsolutePath().toString(), argument)
                .redirectErrorStream(true);
        builder.environment().remove("EMPACK_BOOTSTRAP_CONTROL");
        Process process = builder.start();
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        AtomicReference<IOException> failure = new AtomicReference<>();
        Thread reader = new Thread(() -> {
            try (InputStream input = process.getInputStream()) {
                byte[] buffer = new byte[1024];
                int count;
                while ((count = input.read(buffer)) >= 0) {
                    if (output.size() + count > 8192) {
                        throw new IOException("Engine version output exceeds 8 KiB");
                    }
                    output.write(buffer, 0, count);
                }
            } catch (IOException error) {
                failure.set(error);
            }
        }, "empack-version");
        reader.setDaemon(true);
        reader.start();
        try {
            if (!process.waitFor(10, TimeUnit.SECONDS)) throw new IOException("Engine version probe timed out");
            reader.join(1000);
            if (reader.isAlive()) throw new IOException("Engine version output did not close");
            if (failure.get() != null) throw failure.get();
            String actual = new String(output.toByteArray(), StandardCharsets.UTF_8).trim();
            if (process.exitValue() != 0) throw new IOException("Native engine probe failed: " + actual);
            return actual;
        } finally {
            if (process.isAlive()) {
                process.destroyForcibly();
                process.waitFor();
            }
        }
    }

    static int execute(List<String> command) throws IOException, InterruptedException {
        Integer completed = null;
        try (Control control = new Control()) {
            ProcessBuilder builder = new ProcessBuilder(command).inheritIO();
            builder.environment().put("EMPACK_BOOTSTRAP_CONTROL", control.descriptor());
            Process process = builder.start();
            try {
                completed = process.waitFor();
                return completed;
            } catch (InterruptedException interrupted) {
                IOException cancellationFailure = null;
                try {
                    control.cancel();
                } catch (IOException failure) {
                    cancellationFailure = failure;
                    try {
                        control.close();
                    } catch (IOException cleanup) {
                        failure.addSuppressed(cleanup);
                    }
                }
                int result = waitForRetirement(process);
                completed = result;
                Thread.currentThread().interrupt();
                if (cancellationFailure != null) {
                    throw new IOException("Native empack exited " + result
                            + "; bootstrap cancellation connection failed", cancellationFailure);
                }
                return result;
            }
        } catch (IOException failure) {
            if (completed != null) {
                throw new IOException("Native empack exited " + completed
                        + "; bootstrap connection failed after engine completion", failure);
            }
            throw failure;
        }
    }

    private static int waitForRetirement(Process process) {
        boolean interrupted = false;
        try {
            while (true) {
                try {
                    return process.waitFor();
                } catch (InterruptedException ignored) {
                    interrupted = true;
                }
            }
        } finally {
            if (interrupted) Thread.currentThread().interrupt();
        }
    }

    private static final class Control implements AutoCloseable {
        private final ServerSocket server;
        private final String nonce;
        private Socket peer;
        private boolean closed;
        private boolean cancelled;

        Control() throws IOException {
            byte[] random = new byte[32];
            new SecureRandom().nextBytes(random);
            nonce = Digests.hex(random);
            server = new ServerSocket(0, 16, InetAddress.getByName("127.0.0.1"));
            server.setSoTimeout(250);
            Thread accept = new Thread(this::accept, "empack-bootstrap-control");
            accept.setDaemon(true);
            accept.start();
        }

        String descriptor() {
            return server.getLocalPort() + ":" + nonce;
        }

        private void accept() {
            while (!server.isClosed()) {
                Socket candidate = null;
                try {
                    candidate = server.accept();
                    candidate.setSoTimeout(1000);
                    ByteArrayOutputStream hello = new ByteArrayOutputStream();
                    InputStream input = candidate.getInputStream();
                    int value;
                    while (hello.size() < 128 && (value = input.read()) != -1 && value != '\n') {
                        hello.write(value);
                    }
                    byte[] expected = ("empack-bootstrap/1 " + nonce).getBytes(StandardCharsets.US_ASCII);
                    if (!MessageDigest.isEqual(hello.toByteArray(), expected)) {
                        candidate.close();
                        continue;
                    }
                    synchronized (this) {
                        if (closed) {
                            candidate.close();
                            return;
                        }
                        peer = candidate;
                        peer.getOutputStream().write("READY\n".getBytes(StandardCharsets.US_ASCII));
                        if (cancelled) peer.getOutputStream().write('C');
                        peer.getOutputStream().flush();
                    }
                    return;
                } catch (SocketTimeoutException timeout) {
                    closeCandidate(candidate);
                } catch (SocketException stopped) {
                    closeCandidate(candidate);
                    if (!server.isClosed()) {
                        System.err.println("empack-bootstrap: cancellation connection failed: " + stopped.getMessage());
                    }
                    return;
                } catch (IOException failure) {
                    closeCandidate(candidate);
                    // A malformed local connection must not replace the authenticated
                    // native engine. Its own bounded handshake determines admission.
                }
            }
        }

        private static void closeCandidate(Socket candidate) {
            if (candidate != null) {
                try {
                    candidate.close();
                } catch (IOException ignored) {
                    // The native engine treats connection loss as cancellation.
                }
            }
        }

        synchronized void cancel() throws IOException {
            cancelled = true;
            // Keep listening if cancellation races native startup: authenticate
            // first, then deliver cancellation before waiting for its outcome.
            if (peer != null) {
                peer.getOutputStream().write('C');
                peer.getOutputStream().flush();
            }
        }

        @Override
        public synchronized void close() throws IOException {
            if (closed) return;
            closed = true;
            try {
                if (peer != null) peer.close();
            } finally {
                server.close();
            }
        }
    }
}
