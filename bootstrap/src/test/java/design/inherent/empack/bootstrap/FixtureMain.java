package design.inherent.empack.bootstrap;

import java.nio.file.Path;
import java.nio.file.Paths;
import java.net.URI;
import java.util.Collections;

/** Nonshipping harness for the same archive/catalog implementation production uses. */
public final class FixtureMain {
    public static void main(String[] args) throws Exception {
        if (args[0].equals("unpack")) {
            Platform platform = args[1].equals("current") ? Platform.current()
                    : new Platform(args[1], args[1].contains("windows") ? "empack.exe" : "empack",
                            args[1].contains("windows") ? "zip" : "tar.gz");
            Path executable = EngineArchive.unpack(Paths.get(args[2]), Paths.get(args[3]), platform);
            System.out.println(Digests.sha256(executable));
        } else if (args[0].equals("catalog")) {
            EngineCatalog.Asset asset = new EngineCatalog(new TrustedHttp()).find(args[1], Platform.current());
            System.out.println(asset.name + " " + asset.size + " " + asset.sha256);
        } else if (args[0].equals("fetch")) {
            URI source = URI.create(args[1]);
            if (!source.getHost().equals("localhost")) throw new IllegalArgumentException("Fixture requires localhost HTTPS");
            TrustedHttp http = new TrustedHttp(Collections.singleton("localhost"), Long.parseLong(args[2]));
            Thread caller = new Thread(() -> {
                try {
                    http.metadata(source);
                    throw new AssertionError("Expected acquisition to stop");
                } catch (java.io.IOException stopped) {
                    System.out.println(stopped.getMessage());
                }
            });
            caller.start();
            if (args[3].equals("interrupt")) {
                Thread.sleep(1000);
                caller.interrupt();
            }
            caller.join();
        } else if (args[0].equals("resolve")) {
            URI origin = URI.create(args[1]);
            if (!origin.getHost().equals("localhost")) throw new IllegalArgumentException("Fixture requires localhost HTTPS");
            TrustedHttp http = new TrustedHttp(Collections.singleton("localhost"));
            EngineCatalog catalog = new EngineCatalog(http, origin.resolve("api/releases/tags/"), origin.resolve("assets/"));
            EngineCache cache = new EngineCache(Paths.get(args[2]), http, catalog);
            System.out.println(cache.resolve(args[3], Platform.current(), Boolean.parseBoolean(args[4])));
        } else {
            throw new IllegalArgumentException("Unknown bootstrap fixture mode");
        }
    }
}
