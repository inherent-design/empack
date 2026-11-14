package design.inherent.empack.bootstrap;

import com.google.gson.Gson;
import java.io.Reader;
import java.nio.charset.Charset;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Nonshipping diagnostic: observe JVM ingress separately from ProcessBuilder. */
public final class EncodingProbe {
    public static void main(String[] args) throws Exception {
        String[] values;
        if (args[0].equals("file")) {
            // Controlled synthetic fixture only; never a production invocation parser.
            try (Reader reader = Files.newBufferedReader(Paths.get(args[2]), StandardCharsets.UTF_8)) {
                values = new Gson().fromJson(reader, String[].class);
            }
        } else if (args[0].equals("literal")) {
            values = Arrays.copyOfRange(args, 2, args.length);
        } else {
            throw new IllegalArgumentException("Expected literal or file diagnostic route");
        }
        Map<String, Object> record = new LinkedHashMap<>();
        for (String property : new String[] { "java.runtime.version", "java.vendor", "os.name", "os.arch",
                "file.encoding", "native.encoding", "sun.jnu.encoding" }) {
            // Internal encoding properties are observations, not configurable product APIs.
            record.put(property, System.getProperty(property));
        }
        record.put("default_charset", Charset.defaultCharset().name());
        for (String variable : new String[] { "LANG", "LC_ALL", "LC_CTYPE" }) {
            record.put(variable, System.getenv(variable));
        }
        record.put("jvm_arguments", values);
        System.err.write(("ENCODING_PROBE=" + new Gson().toJson(record) + "\n").getBytes(StandardCharsets.UTF_8));
        List<String> command = new ArrayList<>();
        command.add(args[1]);
        command.add("--fixture-argv");
        command.addAll(Arrays.asList(values));
        // Deliberately exercise the JVM's ordinary native handoff, independently
        // of any bootstrap transport under development.
        System.exit(new ProcessBuilder(command).inheritIO().start().waitFor());
    }
}
