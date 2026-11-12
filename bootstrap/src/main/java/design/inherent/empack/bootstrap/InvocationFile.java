package design.inherent.empack.bootstrap;

import com.google.gson.Strictness;
import com.google.gson.stream.JsonReader;
import com.google.gson.stream.JsonToken;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.StringReader;
import java.nio.ByteBuffer;
import java.nio.channels.SeekableByteChannel;
import java.nio.charset.CharacterCodingException;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.nio.file.attribute.BasicFileAttributes;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/** Static distribution input only. Runtime credentials remain on the supplied argv. */
final class InvocationFile {
    static final int MAX_BYTES = 1024 * 1024;
    static final int MAX_ARGUMENTS = 4096;
    static final int MAX_ARGUMENT_LENGTH = 65536;
    final String version;
    final List<String> arguments;
    final boolean appendArguments;
    final Path directory;

    private InvocationFile(String version, List<String> arguments, boolean appendArguments, Path directory) {
        this.version = version;
        this.arguments = arguments;
        this.appendArguments = appendArguments;
        this.directory = directory;
    }

    static InvocationFile read(Path requested) throws IOException {
        Path file = requested.toAbsolutePath().normalize();
        Path tools = file.getParent();
        if (tools == null || tools.getParent() == null || !tools.getFileName().toString().equals(".empack-tools")
                || !file.getFileName().toString().matches("[A-Za-z0-9_-]+\\.json")) {
            throw new IOException("Invocation file must be an ASCII-named JSON file inside .empack-tools");
        }
        // Canonicalize the distribution base, not the tool directory or file:
        // neither of those may redirect the input through a link/reparse point.
        Path directory = tools.getParent().toRealPath();
        tools = directory.resolve(".empack-tools");
        file = tools.resolve(file.getFileName());
        BasicFileAttributes parent = attributes(tools, true);
        BasicFileAttributes before = attributes(file, false);
        if (before.size() > MAX_BYTES) throw invalid("record exceeds 1 MiB");
        ByteArrayOutputStream captured = new ByteArrayOutputStream();
        try (SeekableByteChannel input = Files.newByteChannel(file, StandardOpenOption.READ, LinkOption.NOFOLLOW_LINKS)) {
            ByteBuffer chunk = ByteBuffer.allocate(8192);
            while (input.read(chunk) != -1) {
                chunk.flip();
                if (chunk.remaining() > MAX_BYTES - captured.size()) throw invalid("record exceeds 1 MiB");
                captured.write(chunk.array(), 0, chunk.remaining());
                chunk.clear();
            }
        }
        if (!same(parent, attributes(tools, true)) || !same(before, attributes(file, false))) {
            throw invalid("input changed while being captured");
        }
        String text;
        try {
            text = StandardCharsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT)
                    .onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(captured.toByteArray())).toString();
        } catch (CharacterCodingException malformed) {
            throw invalid("record is not strict UTF-8");
        }
        return parse(text, directory);
    }

    private static BasicFileAttributes attributes(Path path, boolean directory) throws IOException {
        BasicFileAttributes result = Files.readAttributes(path, BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
        if (result.isSymbolicLink() || result.isOther() || (directory ? !result.isDirectory() : !result.isRegularFile())
                || !path.toRealPath().equals(path)) {
            throw invalid("tool directory and record must be ordinary unlinked filesystem objects");
        }
        return result;
    }

    private static boolean same(BasicFileAttributes before, BasicFileAttributes after) {
        return java.util.Objects.equals(before.fileKey(), after.fileKey())
                && before.size() == after.size() && before.lastModifiedTime().equals(after.lastModifiedTime());
    }

    private static InvocationFile parse(String text, Path directory) throws IOException {
        String version = null;
        List<String> arguments = null;
        Boolean append = null;
        boolean schema = false;
        Set<String> fields = new HashSet<>();
        try (JsonReader reader = new JsonReader(new StringReader(text))) {
            reader.setStrictness(Strictness.STRICT);
            reader.beginObject();
            while (reader.hasNext()) {
                String field = reader.nextName();
                if (!fields.add(field)) throw invalid("duplicate field");
                switch (field) {
                    case "schema":
                        schema = string(reader).equals("empack-bootstrap-invocation/1");
                        if (!schema) throw invalid("unsupported schema");
                        break;
                    case "engine_version":
                        version = string(reader);
                        break;
                    case "arguments":
                        arguments = new ArrayList<>();
                        reader.beginArray();
                        while (reader.hasNext()) {
                            if (arguments.size() == MAX_ARGUMENTS) throw invalid("too many static arguments");
                            arguments.add(string(reader));
                        }
                        reader.endArray();
                        break;
                    case "append_arguments":
                        if (reader.peek() != JsonToken.BOOLEAN) throw invalid("append_arguments must be boolean");
                        append = reader.nextBoolean();
                        break;
                    default:
                        throw invalid("unsupported field");
                }
            }
            reader.endObject();
            if (reader.peek() != JsonToken.END_DOCUMENT) throw invalid("trailing content");
        } catch (IllegalStateException | com.google.gson.JsonParseException malformed) {
            throw invalid("unexpected JSON shape");
        } catch (InvalidInvocation refused) {
            throw refused;
        } catch (IOException malformed) {
            // Parser exceptions can quote input. Do not echo record contents.
            throw invalid("malformed JSON");
        }
        if (!schema || version == null || arguments == null || arguments.isEmpty() || append == null) {
            throw invalid("schema, engine_version, nonempty arguments and append_arguments are required");
        }
        return new InvocationFile(version, arguments, append, directory);
    }

    private static String string(JsonReader reader) throws IOException {
        if (reader.peek() != JsonToken.STRING) throw invalid("expected a string");
        String value = reader.nextString();
        if (value.length() > MAX_ARGUMENT_LENGTH || value.indexOf('\0') >= 0) throw invalid("invalid argument length or NUL");
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            if (Character.isHighSurrogate(ch)) {
                if (++i >= value.length() || !Character.isLowSurrogate(value.charAt(i))) throw invalid("invalid Unicode string");
            } else if (Character.isLowSurrogate(ch)) throw invalid("invalid Unicode string");
        }
        return value;
    }

    private static IOException invalid(String reason) {
        return new InvalidInvocation(reason);
    }

    private static final class InvalidInvocation extends IOException {
        private static final long serialVersionUID = 1L;
        InvalidInvocation(String reason) {
            super("Invalid bootstrap invocation file: " + reason);
        }
    }
}
