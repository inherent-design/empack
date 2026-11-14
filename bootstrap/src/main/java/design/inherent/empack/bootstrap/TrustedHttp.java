package design.inherent.empack.bootstrap;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InterruptedIOException;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URI;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.util.Arrays;
import java.util.HashSet;
import java.util.Locale;
import java.util.Set;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.FutureTask;
import java.util.concurrent.TimeoutException;
import java.util.concurrent.atomic.AtomicReference;

/** Official engine acquisition only; no pack credentials or process-global TLS changes. */
final class TrustedHttp {
    private final Set<String> hosts;
    private final long timeoutMillis;

    TrustedHttp() {
        hosts = new HashSet<>(Arrays.asList("api.github.com", "github.com",
                "objects.githubusercontent.com", "release-assets.githubusercontent.com"));
        timeoutMillis = 180000;
    }

    // The fixture runner supplies an isolated trusted HTTPS origin; production Main
    // never accepts a pack-provided server or a switch disabling certificate checks.
    TrustedHttp(Set<String> hosts) {
        this(hosts, 180000);
    }

    TrustedHttp(Set<String> hosts, long timeoutMillis) {
        this.hosts = new HashSet<>(hosts);
        this.timeoutMillis = timeoutMillis;
    }

    byte[] metadata(URI source) throws IOException {
        ByteArrayOutputStream result = new ByteArrayOutputStream();
        transfer(source, result, 4 * 1024 * 1024);
        return result.toByteArray();
    }

    long download(URI source, Path candidate, long expectedSize) throws IOException {
        if (expectedSize <= 0 || expectedSize > 64L * 1024 * 1024) {
            throw new IOException("Engine archive exceeds the 64 MiB acquisition limit");
        }
        try (OutputStream output = Files.newOutputStream(candidate, StandardOpenOption.CREATE_NEW,
                StandardOpenOption.WRITE)) {
            long observed = transfer(source, output, expectedSize);
            if (observed != expectedSize) throw new IOException("Engine archive length differs from official metadata");
            return observed;
        }
    }

    private long transfer(URI source, OutputStream output, long maximum) throws IOException {
        // DNS, TLS and response headers must share the same interruptible deadline
        // as the body. Socket read timeouts alone do not bound a trickling header.
        AtomicReference<HttpURLConnection> active = new AtomicReference<>();
        final class Sink extends OutputStream {
            private boolean closed;
            @Override public synchronized void write(int value) throws IOException {
                if (closed) throw new IOException("Engine acquisition has ended");
                output.write(value);
            }
            @Override public synchronized void write(byte[] bytes, int offset, int length) throws IOException {
                if (closed) throw new IOException("Engine acquisition has ended");
                output.write(bytes, offset, length);
            }
            @Override public synchronized void close() { closed = true; }
        }
        Sink sink = new Sink();
        long deadline = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(timeoutMillis);
        FutureTask<Long> request = new FutureTask<>(() -> transfer(source, sink, maximum, deadline, active));
        Thread worker = new Thread(request, "empack-bootstrap-https");
        worker.setDaemon(true);
        worker.start();
        try {
            return request.get(timeoutMillis, TimeUnit.MILLISECONDS);
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
            InterruptedIOException failure = new InterruptedIOException("Engine acquisition interrupted");
            failure.initCause(interrupted);
            throw failure;
        } catch (TimeoutException expired) {
            throw new IOException("Engine acquisition deadline exceeded", expired);
        } catch (ExecutionException failed) {
            if (failed.getCause() instanceof IOException) throw (IOException) failed.getCause();
            throw new IOException("Engine acquisition failed", failed.getCause());
        } finally {
            // Stop writes before the owner closes/removes its private candidate.
            // URLConnection.disconnect can itself wait for a blocked reader; it
            // must not hold the caller's cancellation or cleanup path hostage.
            sink.close();
            request.cancel(true);
            Thread disconnect = new Thread(() -> {
                HttpURLConnection connection = active.get();
                if (connection != null) connection.disconnect();
            }, "empack-bootstrap-https-close");
            disconnect.setDaemon(true);
            disconnect.start();
        }
    }

    private long transfer(URI source, OutputStream output, long maximum, long deadline,
                          AtomicReference<HttpURLConnection> active) throws IOException {
        URI current = source;
        for (int redirects = 0; redirects <= 5; redirects++) {
            if (Thread.currentThread().isInterrupted()) throw new IOException("Engine acquisition interrupted");
            if (System.nanoTime() >= deadline) throw new IOException("Engine acquisition deadline exceeded");
            if (!"https".equals(current.getScheme()) || current.getHost() == null
                    || !hosts.contains(current.getHost().toLowerCase(Locale.ROOT))
                    || current.getUserInfo() != null || current.getFragment() != null
                    || (current.getPort() != -1 && current.getPort() != 443
                        && !current.getHost().equals("localhost"))) {
                throw new IOException("Engine acquisition refused an unsupported HTTPS origin");
            }
            HttpURLConnection connection = (HttpURLConnection) current.toURL().openConnection();
            active.set(connection);
            connection.setInstanceFollowRedirects(false);
            connection.setConnectTimeout(10000);
            connection.setReadTimeout(15000);
            connection.setRequestProperty("User-Agent", "empack-bootstrap");
            connection.setRequestProperty("Accept", "application/vnd.github+json");
            connection.setRequestProperty("Accept-Encoding", "identity");
            try {
                int code = connection.getResponseCode();
                if (code == 301 || code == 302 || code == 303 || code == 307 || code == 308) {
                    String location = connection.getHeaderField("Location");
                    if (location == null) throw new IOException("Engine acquisition redirect has no destination");
                    try {
                        current = current.resolve(location);
                    } catch (IllegalArgumentException malformed) {
                        throw new IOException("Engine acquisition received a malformed redirect", malformed);
                    }
                    continue;
                }
                if (code != 200) {
                    throw new IOException("Engine acquisition returned HTTP " + code + " from " + current.getHost());
                }
                long declared = connection.getContentLengthLong();
                if (declared > maximum) throw new IOException("Engine response exceeds its byte allowance");
                long total = 0;
                byte[] buffer = new byte[16384];
                try (InputStream input = connection.getInputStream()) {
                    int count;
                    while ((count = input.read(buffer)) != -1) {
                        if (Thread.currentThread().isInterrupted()) throw new IOException("Engine acquisition interrupted");
                        if (System.nanoTime() >= deadline) throw new IOException("Engine acquisition deadline exceeded");
                        total += count;
                        if (total > maximum) throw new IOException("Engine response exceeds its byte allowance");
                        output.write(buffer, 0, count);
                    }
                }
                if (declared >= 0 && total != declared) throw new IOException("Engine response was truncated");
                return total;
            } finally {
                connection.disconnect();
            }
        }
        throw new IOException("Engine acquisition exceeded five HTTPS redirects");
    }
}
