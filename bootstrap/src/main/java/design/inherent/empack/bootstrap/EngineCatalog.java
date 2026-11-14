package design.inherent.empack.bootstrap;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.google.gson.JsonParseException;
import java.io.IOException;
import java.net.URI;
import java.nio.charset.StandardCharsets;

/** GitHub's HTTPS API supplies the expected digest; release assets supply the bytes. */
final class EngineCatalog {
    private final TrustedHttp http;
    private final URI repository;
    private final URI downloads;

    EngineCatalog(TrustedHttp http) {
        this(http, URI.create("https://api.github.com/repos/inherent-design/empack/releases/tags/"),
                URI.create("https://github.com/inherent-design/empack/releases/download/"));
    }

    EngineCatalog(TrustedHttp http, URI repository, URI downloads) {
        this.http = http;
        this.repository = repository;
        this.downloads = downloads;
    }

    Asset find(String version, Platform platform) throws IOException {
        try {
            URI endpoint = repository.resolve("v" + version);
            JsonObject release = JsonParser.parseString(
                    new String(http.metadata(endpoint), StandardCharsets.UTF_8)).getAsJsonObject();
            if (!text(release, "tag_name").equals("v" + version)
                    || !release.has("draft") || !release.get("draft").isJsonPrimitive()
                    || !release.getAsJsonPrimitive("draft").isBoolean()
                    || release.get("draft").getAsBoolean()) {
                throw new IOException("Official engine metadata does not identify the requested published release");
            }
            String name = platform.archive(version);
            JsonElement assetList = release.get("assets");
            if (assetList == null || !assetList.isJsonArray()) {
                throw new IOException("Official engine metadata requires an archive list");
            }
            JsonArray assets = assetList.getAsJsonArray();
            Asset found = null;
            for (JsonElement item : assets) {
                JsonObject asset = item.getAsJsonObject();
                if (!name.equals(text(asset, "name"))) continue;
                if (found != null) throw new IOException("Official engine metadata repeats the selected archive");
                String digest = text(asset, "digest");
                String encodedSize = asset.get("size").toString();
                if (!encodedSize.matches("[1-9][0-9]*")) throw new IOException("Official engine size must be a positive integer");
                long size = Long.parseLong(encodedSize);
                if (!digest.matches("sha256:[0-9a-f]{64}") || size <= 0 || size > 64L * 1024 * 1024) {
                    throw new IOException("Official engine metadata lacks an admitted SHA-256 digest and size");
                }
                // Ignore URLs provided inside the response. The production repository,
                // exact tag and known target determine the only initial download URL.
                found = new Asset(name, digest.substring(7), size,
                        downloads.resolve("v" + version + "/" + name));
            }
            if (found == null) throw new IOException("The requested release has no engine archive for " + platform.target);
            return found;
        } catch (JsonParseException | IllegalStateException | IllegalArgumentException | NullPointerException malformed) {
            throw new IOException("Malformed official engine release metadata", malformed);
        }
    }

    private static String text(JsonObject object, String key) throws IOException {
        JsonElement value = object.get(key);
        if (value == null || !value.isJsonPrimitive() || !value.getAsJsonPrimitive().isString()) {
            throw new IOException("Official engine metadata requires a string field: " + key);
        }
        return value.getAsString();
    }

    static final class Asset {
        final String name;
        final String sha256;
        final long size;
        final URI url;

        Asset(String name, String sha256, long size, URI url) {
            this.name = name;
            this.sha256 = sha256;
            this.size = size;
            this.url = url;
        }
    }
}
