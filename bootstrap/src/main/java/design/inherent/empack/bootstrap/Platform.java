package design.inherent.empack.bootstrap;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.Locale;

final class Platform {
    final String target;
    final String executable;
    final String extension;

    Platform(String target, String executable, String extension) {
        this.target = target;
        this.executable = executable;
        this.extension = extension;
    }

    static Platform current() throws IOException {
        String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        String architecture = System.getProperty("os.arch", "").toLowerCase(Locale.ROOT);
        String cpu;
        if (architecture.equals("amd64") || architecture.equals("x86_64")) cpu = "x86_64";
        else if (architecture.equals("aarch64") || architecture.equals("arm64")) cpu = "aarch64";
        else throw new IOException("No empack engine for JVM architecture " + architecture);
        if (os.startsWith("windows") && cpu.equals("x86_64")) {
            return new Platform("x86_64-pc-windows-msvc", "empack.exe", "zip");
        }
        if (os.equals("mac os x") || os.equals("macos")) {
            return new Platform(cpu + "-apple-darwin", "empack", "tar.gz");
        }
        if (os.equals("linux")) {
            String musl = cpu.equals("x86_64") ? "x86_64" : "aarch64";
            if (Files.exists(Paths.get("/lib/ld-musl-" + musl + ".so.1"))) {
                throw new IOException("Official empack Linux engines require glibc; this host exposes musl");
            }
            return new Platform(cpu + "-unknown-linux-gnu", "empack", "tar.gz");
        }
        throw new IOException("No official empack engine for " + os + " / " + architecture);
    }

    String archive(String version) {
        return "empack-" + version + "-" + target + "." + extension;
    }
}
