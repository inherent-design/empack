package design.inherent.empack.bootstrap;

import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.Files;
import java.nio.charset.StandardCharsets;
import java.net.URI;
import java.util.Collections;

/** Nonshipping archive/cache harness and argument-boundary feasibility probe. */
public final class FixtureMain {
    public static void main(String[] args) throws Exception {
        if (args[0].equals("captured-invocation")) {
            Main.Options options = Main.Options.parse(java.util.Arrays.copyOfRange(args, 2, args.length));
            Path record = Paths.get(args[1]);
            Path replacement = record.resolveSibling("replacement.json");
            Files.write(replacement, "invalid replacement must not execute".getBytes(StandardCharsets.UTF_8));
            Files.move(replacement, record, java.nio.file.StandardCopyOption.REPLACE_EXISTING);
            System.exit(Main.run(options));
        } else if (args[0].equals("effective-write")) {
            java.nio.file.attribute.AclEntry.Builder entry = java.nio.file.attribute.AclEntry.newBuilder()
                    .setType(java.nio.file.attribute.AclEntryType.ALLOW)
                    .setPrincipal(() -> "fixture-outside-writer")
                    .setPermissions(java.nio.file.attribute.AclEntryPermission.WRITE_DATA);
            java.util.Set<java.nio.file.attribute.AclEntryFlag> flags = java.util.EnumSet.noneOf(
                    java.nio.file.attribute.AclEntryFlag.class);
            for (int i = 1; i < args.length; i++) flags.add(java.nio.file.attribute.AclEntryFlag.valueOf(args[i]));
            entry.setFlags(flags);
            System.out.println(EngineCache.writes(entry.build()));
        } else if (args[0].equals("cache-authority")) {
            java.util.Map<String, String> observation = new java.util.LinkedHashMap<>();
            for (String kind : new String[] { "home", "cache" }) {
                Path path = kind.equals("home") ? Paths.get(System.getProperty("user.home")) : Paths.get(args[1]);
                observation.put(kind + "_owner", Files.getOwner(path).getName());
                java.nio.file.attribute.AclFileAttributeView view = Files.getFileAttributeView(
                        path, java.nio.file.attribute.AclFileAttributeView.class);
                String acl = view == null ? "no ACL view" : view.getAcl().toString();
                observation.put(kind + "_acl", acl.substring(0, Math.min(acl.length(), 8192)));
            }
            System.out.write((new com.google.gson.Gson().toJson(observation) + "\n").getBytes(StandardCharsets.UTF_8));
        } else if (args[0].equals("unpack")) {
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
