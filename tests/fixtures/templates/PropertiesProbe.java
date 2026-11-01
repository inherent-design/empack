import java.io.ByteArrayInputStream;
import java.io.StringReader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.Properties;

// Read actual archive bytes through both JDK entry points. No empack encoder is
// called here; the caller compares Unicode code points with the original input.
class PropertiesProbe {
    public static void main(String[] args) throws Exception {
        if (args.length == 0) {
            throw new IllegalArgumentException("Expected captured properties files");
        }
        System.err.println("Properties probe: Java " + Runtime.version());
        for (String arg : args) {
            byte[] bytes = Files.readAllBytes(Path.of(arg));
            Properties stream = new Properties();
            stream.load(new ByteArrayInputStream(bytes));
            Properties reader = new Properties();
            reader.load(new StringReader(new String(bytes, StandardCharsets.UTF_8)));
            System.out.println("{\"stream\":"
                    + Arrays.toString(stream.getProperty("motd").codePoints().toArray())
                    + ",\"reader\":"
                    + Arrays.toString(reader.getProperty("motd").codePoints().toArray())
                    + ",\"stream_keys\":" + stream.size()
                    + ",\"reader_keys\":" + reader.size()
                    + ",\"stream_empty\":" + "".equals(stream.getProperty("empty"))
                    + ",\"reader_empty\":" + "".equals(reader.getProperty("empty")) + "}");
        }
    }
}
