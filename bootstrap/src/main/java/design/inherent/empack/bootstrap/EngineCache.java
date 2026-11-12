package design.inherent.empack.bootstrap;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.nio.channels.FileChannel;
import java.nio.channels.FileLock;
import java.nio.channels.OverlappingFileLockException;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.FileAlreadyExistsException;
import java.nio.file.FileVisitResult;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.SimpleFileVisitor;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.nio.file.attribute.BasicFileAttributes;
import java.nio.file.attribute.AclEntry;
import java.nio.file.attribute.AclEntryFlag;
import java.nio.file.attribute.AclEntryPermission;
import java.nio.file.attribute.AclEntryType;
import java.nio.file.attribute.AclFileAttributeView;
import java.nio.file.attribute.PosixFilePermission;
import java.nio.file.attribute.PosixFilePermissions;
import java.nio.file.attribute.UserPrincipal;
import java.nio.file.attribute.FileAttribute;
import java.util.List;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Locale;
import java.util.EnumSet;
import java.util.HashSet;
import java.util.Set;
import java.util.Properties;
import java.util.UUID;
import java.util.concurrent.TimeUnit;

/** Private immutable engine objects; the per-version lock never spans delegated execution. */
final class EngineCache {
    private final Path root;
    private final TrustedHttp http;
    private final EngineCatalog catalog;
    private final UserPrincipal owner;
    private final Set<UserPrincipal> homeWriters = new HashSet<>();
    private final FileAttribute<List<AclEntry>> directoryAcl;
    private static final Set<AclEntryPermission> WRITE_PERMISSIONS = EnumSet.of(
            AclEntryPermission.WRITE_DATA, AclEntryPermission.APPEND_DATA,
            AclEntryPermission.WRITE_ATTRIBUTES, AclEntryPermission.WRITE_NAMED_ATTRS,
            AclEntryPermission.DELETE, AclEntryPermission.DELETE_CHILD,
            AclEntryPermission.WRITE_ACL, AclEntryPermission.WRITE_OWNER);

    EngineCache(Path root, TrustedHttp http, EngineCatalog catalog) throws IOException {
        this.root = root.toAbsolutePath().normalize();
        this.http = http;
        this.catalog = catalog;
        Path home = Paths.get(System.getProperty("user.home"));
        owner = Files.getOwner(home);
        homeWriters.add(owner);
        AclFileAttributeView homeAcl = Files.getFileAttributeView(home, AclFileAttributeView.class);
        if (homeAcl == null) {
            directoryAcl = null;
        } else {
            List<AclEntry> entries = new ArrayList<>();
            for (AclEntry entry : homeAcl.getAcl()) {
                if (entry.type() == AclEntryType.ALLOW && writes(entry)) homeWriters.add(entry.principal());
                if (!entry.flags().contains(AclEntryFlag.INHERIT_ONLY) || entry.type() == AclEntryType.DENY) {
                    entries.add(AclEntry.newBuilder(entry)
                            .setFlags(AclEntryFlag.FILE_INHERIT, AclEntryFlag.DIRECTORY_INHERIT).build());
                }
            }
            entries.add(AclEntry.newBuilder().setType(AclEntryType.ALLOW).setPrincipal(owner)
                    .setPermissions(EnumSet.allOf(AclEntryPermission.class))
                    .setFlags(AclEntryFlag.FILE_INHERIT, AclEntryFlag.DIRECTORY_INHERIT).build());
            List<AclEntry> captured = Collections.unmodifiableList(entries);
            directoryAcl = new FileAttribute<List<AclEntry>>() {
                @Override public String name() { return "acl:acl"; }
                @Override public List<AclEntry> value() { return captured; }
            };
        }
    }

    static Path defaultRoot() throws IOException {
        String override = System.getenv("EMPACK_BOOTSTRAP_CACHE");
        if (override != null && !override.isEmpty()) return absoluteEnvironmentPath(override);
        String home = System.getProperty("user.home");
        if (home == null || home.isEmpty()) throw new IOException("No private user home for the engine cache");
        String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        if (os.startsWith("windows")) {
            String local = System.getenv("LOCALAPPDATA");
            return (local == null ? Paths.get(home, "AppData", "Local") : absoluteEnvironmentPath(local))
                    .resolve("empack").resolve("engines");
        }
        if (os.equals("mac os x") || os.equals("macos")) return Paths.get(home, "Library", "Caches", "empack", "engines");
        String xdg = System.getenv("XDG_CACHE_HOME");
        return (xdg == null ? Paths.get(home, ".cache") : absoluteEnvironmentPath(xdg))
                .resolve("empack").resolve("engines");
    }

    private static Path absoluteEnvironmentPath(String value) throws IOException {
        Path path = Paths.get(value);
        if (!path.isAbsolute()) throw new IOException("Engine cache overrides must be absolute paths");
        return path;
    }

    Path resolve(String version, Platform platform, boolean offline) throws IOException, InterruptedException {
        if (version.contains("/") || version.contains("\\") || version.equals(".") || version.equals("..")) {
            throw new IOException("Invalid engine cache version component");
        }
        directory(root);
        // SemVer prerelease spelling is case-sensitive; common filesystems are
        // not. A full content-derived component keeps distinct requests separate.
        Path slot = root.resolve(Digests.sha256(version.getBytes(StandardCharsets.UTF_8)))
                .resolve(platform.target);
        directory(slot);
        Path lockPath = slot.resolve("writer.lock");
        if (Files.exists(lockPath, LinkOption.NOFOLLOW_LINKS)) regular(lockPath);
        try (FileChannel channel = FileChannel.open(lockPath, StandardOpenOption.CREATE,
                StandardOpenOption.READ, StandardOpenOption.WRITE);
             FileLock lock = acquire(channel)) {
            regular(lockPath);
            Path ready = slot.resolve("ready.properties");
            recoverMetadata(slot);
            recoverPending(slot);
            if (Files.exists(ready, LinkOption.NOFOLLOW_LINKS)) return admitted(slot, load(ready), version, platform);
            if (offline) throw new IOException("Requested engine is not cached; offline bootstrap cannot acquire it");
            EngineCatalog.Asset asset = catalog.find(version, platform);
            Path objects = slot.resolve("objects");
            directory(objects);
            Path object = objects.resolve(asset.sha256);
            if (Files.exists(object, LinkOption.NOFOLLOW_LINKS)) {
                directory(object);
                Properties recovered = load(object.resolve("engine.properties"));
                if (!asset.sha256.equals(recovered.getProperty("archive"))) {
                    throw new IOException("Interrupted engine object does not match official metadata");
                }
                Path engine = admitted(slot, recovered, version, platform);
                atomicProperties(ready, recovered);
                return engine;
            }
            String id = UUID.randomUUID().toString();
            String candidateName = "private-" + id;
            Properties pending = new Properties();
            pending.setProperty("kind", "empack-engine-download/1");
            pending.setProperty("candidate", candidateName);
            atomicProperties(slot.resolve("pending.properties"), pending);
            Path candidate = slot.resolve(candidateName);
            directory(candidate);
            boolean published = false;
            try {
                Path archive = candidate.resolve("archive");
                http.download(asset.url, archive, asset.size);
                if (!Digests.sha256(archive).equals(asset.sha256)) {
                    throw new IOException("Engine archive SHA-256 differs from authenticated official metadata");
                }
                Path engine = EngineArchive.unpack(archive, candidate, platform);
                EngineProcess.verifyVersion(engine, version, false);
                EngineProcess.verifyProtocol(engine);
                Properties record = new Properties();
                record.setProperty("kind", "empack-engine/1");
                record.setProperty("version", version);
                record.setProperty("target", platform.target);
                record.setProperty("archive", asset.sha256);
                record.setProperty("executable", Digests.sha256(engine));
                record.setProperty("bytes", Long.toString(Files.size(engine)));
                atomicProperties(candidate.resolve("engine.properties"), record);
                Files.delete(archive);
                Files.move(candidate, object, StandardCopyOption.ATOMIC_MOVE);
                published = true;
                atomicProperties(ready, record);
                Files.deleteIfExists(slot.resolve("pending.properties"));
                return admitted(slot, record, version, platform);
            } finally {
                if (!published && Files.exists(candidate, LinkOption.NOFOLLOW_LINKS)) removeCandidate(candidate);
                if (!published) Files.deleteIfExists(slot.resolve("pending.properties"));
            }
        } catch (AtomicMoveNotSupportedException unsupported) {
            throw new IOException("Engine cache requires atomic publication on its filesystem", unsupported);
        }
    }

    private Path admitted(Path slot, Properties record, String version, Platform platform) throws IOException {
        String archive = record.getProperty("archive", "");
        String digest = record.getProperty("executable", "");
        if (!"empack-engine/1".equals(record.getProperty("kind"))
                || !version.equals(record.getProperty("version")) || !platform.target.equals(record.getProperty("target"))
                || !archive.matches("[0-9a-f]{64}") || !digest.matches("[0-9a-f]{64}")) {
            throw new IOException("Cached engine record is invalid for empack " + version + " (" + platform.target + "); inspect " + slot.resolve("ready.properties"));
        }
        Path object = slot.resolve("objects").resolve(archive);
        if (!Files.isDirectory(object, LinkOption.NOFOLLOW_LINKS)) {
            throw new IOException("Cached engine backing is missing for empack " + version + "; inspect " + object);
        }
        directory(object);
        Path engine = object.resolve(platform.executable);
        regular(engine);
        if (!Files.isExecutable(engine) || Files.size(engine) > EngineArchive.MAX_EXECUTABLE
                || !Long.toString(Files.size(engine)).equals(record.getProperty("bytes"))
                || !Digests.sha256(engine).equals(digest)) {
            throw new IOException("Cached engine is missing, corrupt or not executable; inspect " + engine);
        }
        return engine;
    }

    private static FileLock acquire(FileChannel channel) throws IOException, InterruptedException {
        long until = System.nanoTime() + TimeUnit.SECONDS.toNanos(30);
        do {
            try {
                FileLock lock = channel.tryLock();
                if (lock != null) return lock;
            } catch (OverlappingFileLockException busy) { /* Another invocation owns this slot. */ }
            Thread.sleep(50);
        } while (System.nanoTime() < until);
        throw new IOException("Another bootstrap is acquiring this engine; retry after it completes");
    }

    private void directory(Path directory) throws IOException {
        Path absolute = directory.toAbsolutePath().normalize();
        Path parent = absolute.getParent();
        if (absolute.startsWith(root) && !absolute.equals(root)) directory(parent);
        if (!Files.exists(absolute, LinkOption.NOFOLLOW_LINKS)) {
            if (parent != null && !Files.exists(parent, LinkOption.NOFOLLOW_LINKS)) directory(parent);
            try {
                if (Files.getFileStore(parent).supportsFileAttributeView("posix")) {
                    Files.createDirectory(absolute, PosixFilePermissions.asFileAttribute(
                            PosixFilePermissions.fromString("rwx------")));
                } else {
                    if (directoryAcl == null) throw new IOException("Cannot establish private engine-cache ACLs");
                    // Establish the home trust boundary atomically on creation,
                    // rather than inheriting a temporary parent's default ACL.
                    // Existing objects and competing winners are never repaired.
                    Files.createDirectory(absolute, directoryAcl);
                }
            } catch (FileAlreadyExistsException competingCreator) { /* Revalidate the winner below. */ }
        }
        BasicFileAttributes attributes = Files.readAttributes(absolute, BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
        if (!attributes.isDirectory() || attributes.isSymbolicLink() || attributes.isOther()
                || !absolute.toRealPath().equals(absolute)) {
            throw new IOException("Engine cache directory must not traverse links or filesystem aliases: " + directory);
        }
        privateOwner(absolute);
    }

    private void regular(Path file) throws IOException {
        BasicFileAttributes attributes = Files.readAttributes(file, BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
        if (!attributes.isRegularFile() || attributes.isSymbolicLink() || !file.toRealPath().equals(file.toAbsolutePath().normalize())) {
            throw new IOException("Engine cache contains a linked or nonregular object: " + file);
        }
        privateOwner(file);
    }

    private void privateOwner(Path path) throws IOException {
        boolean posix = Files.getFileStore(path).supportsFileAttributeView("posix");
        UserPrincipal actualOwner = Files.getOwner(path, LinkOption.NOFOLLOW_LINKS);
        // Windows homes and newly created objects can have different owners
        // (for example Administrators and the interactive account). Accept no
        // principal beyond those already trusted to write that home. POSIX
        // ownership remains exact, independently of its write-mode check.
        if (posix ? !actualOwner.equals(owner) : !homeWriters.contains(actualOwner)) {
            throw new IOException("Engine cache object is owned by a different user: " + path);
        }
        if (posix) {
            java.util.Set<PosixFilePermission> mode = Files.getPosixFilePermissions(path, LinkOption.NOFOLLOW_LINKS);
            if (mode.contains(PosixFilePermission.GROUP_WRITE) || mode.contains(PosixFilePermission.OTHERS_WRITE)) {
                throw new IOException("Engine cache object permits writes by another user: " + path);
            }
        } else {
            AclFileAttributeView acl = Files.getFileAttributeView(path, AclFileAttributeView.class, LinkOption.NOFOLLOW_LINKS);
            if (acl == null) throw new IOException("Engine cache filesystem cannot establish private executable permissions");
            for (AclEntry entry : acl.getAcl()) {
                if (entry.type() == AclEntryType.ALLOW && writes(entry) && !homeWriters.contains(entry.principal())) {
                    throw new IOException("Engine cache ACL admits a writer outside the user-home trust boundary: " + path);
                }
            }
        }
    }

    static boolean writes(AclEntry entry) {
        // Inherit-only entries describe descendants, not this observed object.
        if (entry.flags().contains(AclEntryFlag.INHERIT_ONLY)) return false;
        for (AclEntryPermission permission : entry.permissions()) {
            if (WRITE_PERMISSIONS.contains(permission)) return true;
        }
        return false;
    }

    private Properties load(Path path) throws IOException {
        regular(path);
        if (Files.size(path) > 4096) throw new IOException("Engine cache metadata exceeds its byte limit");
        Properties record = new Properties();
        try {
            record.load(new ByteArrayInputStream(Files.readAllBytes(path)));
        } catch (IllegalArgumentException malformed) {
            throw new IOException("Invalid engine cache metadata", malformed);
        }
        return record;
    }

    private static void atomicProperties(Path destination, Properties record) throws IOException {
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        record.store(bytes, "empack engine cache");
        Path temporary = destination.resolveSibling("metadata-" + UUID.randomUUID() + ".tmp");
        try {
            try (FileChannel file = FileChannel.open(temporary, StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE)) {
                ByteBuffer data = ByteBuffer.wrap(bytes.toByteArray());
                while (data.hasRemaining()) file.write(data);
                file.force(true);
            }
            Files.move(temporary, destination, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
        } finally {
            Files.deleteIfExists(temporary);
        }
    }

    private void recoverPending(Path slot) throws IOException {
        Path journal = slot.resolve("pending.properties");
        if (!Files.exists(journal, LinkOption.NOFOLLOW_LINKS)) return;
        Properties record = load(journal);
        String name = record.getProperty("candidate", "");
        if (!"empack-engine-download/1".equals(record.getProperty("kind"))
                || !name.matches("private-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")) {
            throw new IOException("Engine cache has an unrecognized interrupted-download record");
        }
        Path candidate = slot.resolve(name);
        if (Files.exists(candidate, LinkOption.NOFOLLOW_LINKS)) removeCandidate(candidate);
        Files.delete(journal);
    }

    private void recoverMetadata(Path slot) throws IOException {
        try (java.nio.file.DirectoryStream<Path> entries = Files.newDirectoryStream(slot, "metadata-*.tmp")) {
            for (Path entry : entries) {
                if (entry.getFileName().toString().matches("metadata-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\\.tmp")) {
                    regular(entry);
                    Files.delete(entry);
                }
            }
        }
    }

    private void removeCandidate(Path candidate) throws IOException {
        directory(candidate);
        Files.walkFileTree(candidate, new SimpleFileVisitor<Path>() {
            @Override public FileVisitResult visitFile(Path file, BasicFileAttributes attributes) throws IOException {
                Files.delete(file); // Does not follow a link encountered inside this private candidate.
                return FileVisitResult.CONTINUE;
            }
            @Override public FileVisitResult postVisitDirectory(Path dir, IOException failure) throws IOException {
                if (failure != null) throw failure;
                Files.delete(dir);
                return FileVisitResult.CONTINUE;
            }
        });
    }
}
