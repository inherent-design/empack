package design.inherent.empack.bootstrap;

import java.io.FilterInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.RandomAccessFile;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.nio.file.attribute.PosixFilePermissions;
import java.util.Enumeration;
import java.util.HashSet;
import java.util.Set;
import java.util.zip.CRC32;
import java.util.zip.GZIPInputStream;
import org.apache.commons.compress.archivers.tar.TarArchiveEntry;
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream;
import org.apache.commons.compress.archivers.zip.ZipArchiveEntry;
import org.apache.commons.compress.archivers.zip.ZipFile;

/** Extract the one executable from an authenticated, bounded official engine archive. */
final class EngineArchive {
    private static final long MAX_DECODED = 128L * 1024 * 1024;
    static final long MAX_EXECUTABLE = 96L * 1024 * 1024;

    private EngineArchive() {}

    static Path unpack(Path archive, Path directory, Platform platform) throws IOException {
        Inventory inventory = new Inventory(directory, platform.executable);
        if (platform.extension.equals("zip")) {
            admitZipIndex(archive);
            try (ZipFile zip = ZipFile.builder().setPath(archive).get()) {
                Enumeration<ZipArchiveEntry> entries = zip.getEntries();
                while (entries.hasMoreElements()) {
                    ZipArchiveEntry entry = entries.nextElement();
                    int kind = entry.getUnixMode() & 0170000;
                    if (entry.isUnixSymlink() || (kind != 0 && kind != 0100000 && kind != 0040000)
                            || (entry.getExternalAttributes() & 0x400) != 0
                            || (entry.getMethod() != 0 && entry.getMethod() != 8)
                            || !zip.canReadEntryData(entry)) {
                        throw new IOException("Engine ZIP contains a link, special or unsupported entry");
                    }
                    try (InputStream bytes = zip.getInputStream(entry)) {
                        long crc = inventory.entry(entry.getName(), entry.isDirectory(), entry.getSize(), bytes);
                        if (crc != entry.getCrc()) throw new IOException("Engine ZIP member CRC does not match");
                    }
                }
            }
        } else {
            try (InputStream raw = Files.newInputStream(archive);
                 GZIPInputStream gzip = new GZIPInputStream(raw);
                 LimitedInput decoded = new LimitedInput(gzip, MAX_DECODED);
                 TarArchiveInputStream tar = new TarArchiveInputStream(decoded)) {
                TarArchiveEntry entry;
                while ((entry = tar.getNextEntry()) != null) {
                    if ((!entry.isFile() && !entry.isDirectory()) || entry.isSparse()
                            || entry.isSymbolicLink() || entry.isLink()) {
                        throw new IOException("Engine TAR contains a link, special or sparse entry");
                    }
                    inventory.entry(entry.getName(), entry.isDirectory(), entry.getSize(), tar);
                }
                // TAR's zero blocks precede gzip's integrity trailer.
                byte[] buffer = new byte[16384];
                while (decoded.read(buffer) != -1) { /* Validate the complete gzip stream. */ }
            }
        }
        if (!inventory.found) throw new IOException("Official engine archive omits its executable");
        Path executable = directory.resolve(platform.executable);
        verifyTarget(executable, platform);
        if (Files.getFileStore(executable).supportsFileAttributeView("posix")) {
            Files.setPosixFilePermissions(executable, PosixFilePermissions.fromString("r-x------"));
        } else if (!executable.toFile().setExecutable(true, true)) {
            throw new IOException("Cannot establish executable attributes for cached empack");
        }
        return executable;
    }

    private static final class Inventory {
        private final Path directory;
        private final String executable;
        private final Set<String> names = new HashSet<>();
        private long total;
        private int entries;
        private boolean found;

        Inventory(Path directory, String executable) {
            this.directory = directory;
            this.executable = executable;
        }

        long entry(String rawName, boolean isDirectory, long size, InputStream input) throws IOException {
            if (++entries > 16 || rawName.length() > 256 || size < 0 || size > MAX_EXECUTABLE) {
                throw new IOException("Engine archive member exceeds its admission limits");
            }
            String name = rawName.startsWith("./") ? rawName.substring(2) : rawName;
            if (isDirectory) {
                if (!(name.equals(".") || name.isEmpty()) || size != 0 || !names.add(".")) {
                    throw new IOException("Engine archive contains an unexpected directory");
                }
                return 0;
            }
            if (!(name.equals(executable) || name.equals("LICENSE") || name.equals("README.md"))
                    || !names.add(name)) {
                throw new IOException("Engine archive contains an unexpected or duplicate member");
            }
            if (!name.equals(executable) && size > 65536) throw new IOException("Engine auxiliary member is too large");
            CRC32 crc = new CRC32();
            long bytes = 0;
            OutputStream output = name.equals(executable)
                    ? Files.newOutputStream(directory.resolve(executable), StandardOpenOption.CREATE_NEW)
                    : null;
            try {
                byte[] buffer = new byte[16384];
                int count;
                while ((count = input.read(buffer)) != -1) {
                    if (Thread.currentThread().isInterrupted()) throw new IOException("Engine extraction interrupted");
                    bytes += count;
                    total += count;
                    if (bytes > size || total > MAX_DECODED) throw new IOException("Engine archive decoded beyond its limits");
                    crc.update(buffer, 0, count);
                    if (output != null) output.write(buffer, 0, count);
                }
            } finally {
                if (output != null) output.close();
            }
            if (bytes != size) throw new IOException("Engine member length differs from its declaration");
            if (name.equals(executable)) found = true;
            return crc.getValue();
        }
    }

    private static final class LimitedInput extends FilterInputStream {
        private final long maximum;
        private long count;

        LimitedInput(InputStream input, long maximum) { super(input); this.maximum = maximum; }

        private void charge(long bytes) throws IOException {
            if (bytes > 0 && (count += bytes) > maximum) throw new IOException("Engine archive exceeds decoded metadata allowance");
        }

        @Override public int read() throws IOException {
            int result = in.read();
            charge(result == -1 ? 0 : 1);
            return result;
        }

        @Override public int read(byte[] bytes, int offset, int length) throws IOException {
            int result = in.read(bytes, offset, length);
            charge(result);
            return result;
        }

        @Override public long skip(long length) throws IOException {
            long skipped = in.skip(Math.min(length, maximum - count + 1));
            charge(skipped);
            return skipped;
        }
    }

    private static void admitZipIndex(Path archive) throws IOException {
        // Official engine archives are small and do not need ZIP64. Bound central
        // metadata before constructing the library's complete name index.
        try (RandomAccessFile file = new RandomAccessFile(archive.toFile(), "r")) {
            int length = (int) Math.min(file.length(), 65557);
            byte[] tail = new byte[length];
            file.seek(file.length() - length);
            file.readFully(tail);
            ByteBuffer bytes = ByteBuffer.wrap(tail).order(ByteOrder.LITTLE_ENDIAN);
            for (int i = length - 22; i >= 0; i--) {
                if (bytes.getInt(i) != 0x06054b50 || i + 22 + (bytes.getShort(i + 20) & 65535) != length) continue;
                int count = bytes.getShort(i + 10) & 65535;
                long size = bytes.getInt(i + 12) & 0xffffffffL;
                long offset = bytes.getInt(i + 16) & 0xffffffffL;
                if (bytes.getInt(i + 4) != 0 || (bytes.getShort(i + 8) & 65535) != count
                        || count > 16 || size > 32768 || offset + size != file.length() - length + i) {
                    throw new IOException("Engine ZIP index is unsupported or exceeds its metadata limits");
                }
                return;
            }
            throw new IOException("Engine ZIP has no complete supported directory record");
        }
    }

    private static void verifyTarget(Path executable, Platform platform) throws IOException {
        try (RandomAccessFile file = new RandomAccessFile(executable.toFile(), "r")) {
            byte[] header = new byte[64];
            file.readFully(header);
            ByteBuffer bytes = ByteBuffer.wrap(header).order(ByteOrder.LITTLE_ENDIAN);
            boolean arm = platform.target.startsWith("aarch64-");
            if (platform.target.endsWith("-apple-darwin")) {
                if (bytes.getInt(0) == 0xfeedfacf && bytes.getInt(4) == (arm ? 0x0100000c : 0x01000007)) return;
            } else if (platform.target.endsWith("-linux-gnu")) {
                if (bytes.getInt(0) == 0x464c457f && header[4] == 2 && header[5] == 1
                        && bytes.getShort(18) == (arm ? 183 : 62)) return;
            } else if (platform.target.endsWith("-windows-msvc") && bytes.getShort(0) == 0x5a4d) {
                long offset = bytes.getInt(60) & 0xffffffffL;
                if (offset >= 64 && offset < 65536 && offset + 6 <= file.length()) {
                    file.seek(offset);
                    byte[] pe = new byte[6];
                    file.readFully(pe);
                    ByteBuffer signature = ByteBuffer.wrap(pe).order(ByteOrder.LITTLE_ENDIAN);
                    if (signature.getInt(0) == 0x00004550 && (signature.getShort(4) & 65535) == 0x8664) return;
                }
            }
            throw new IOException("Engine executable header does not match requested target " + platform.target);
        }
    }

}
