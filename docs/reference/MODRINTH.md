# Modrinth API Reference

Complete technical reference for the Modrinth API (Labrinth). This document covers all endpoints, parameters, response schemas, rate limits, authentication, and integration patterns.

## Overview

- **API Name**: Labrinth
- **Current Version**: v2.7.0/366f528
- **Base URL (Production)**: `https://api.modrinth.com`
- **Base URL (Staging)**: `https://staging-api.modrinth.com`
- **API Specification**: OpenAPI 3.0.0
- **Terms of Service**: https://modrinth.com/legal/terms
- **Support**: support@modrinth.com | https://support.modrinth.com

### Quick Start

Test your API client by making a GET request to the base URL:

```
GET https://staging-api.modrinth.com/
```

Expected response:
```json
{
  "about": "Welcome traveler!",
  "documentation": "https://docs.modrinth.com",
  "name": "modrinth-labrinth",
  "version": "2.7.0"
}
```

For production, use `api.modrinth.com` instead of `staging-api.modrinth.com`.

---

## Authentication

Modrinth supports two authentication methods: **Personal Access Tokens (PATs)** and **OAuth2**.

### Authentication Headers

All tokens use the `Authorization` header:

```
Authorization: mrp_RNtLRSPmGj2pd1v1ubi52nX7TJJM9sznrmwhAuj511oe4t1jAqAQ3D6Wc8Ic
```

### When Authentication is Required

Authentication is **optional** for most read operations. It is **required** for:

1. **Creating data** - Version creation, project creation, etc.
2. **Modifying data** - Editing projects, updating versions, etc.
3. **Accessing private data** - Draft projects, notifications, emails, payout data

### Scopes

Each authenticated request requires specific scopes. Common scopes include:

- `USER_READ` - Read user information
- `USER_READ_EMAIL` - Read user email addresses
- `USER_WRITE` - Modify user information
- `PROJECT_READ` - Read project information (including private projects)
- `PROJECT_WRITE` - Create and modify projects
- `VERSION_READ` - Read version information (including drafts)
- `VERSION_WRITE` - Create and modify versions
- `NOTIFICATION_READ` - Read notifications
- `NOTIFICATION_WRITE` - Mark notifications as read
- `PAYOUT_READ` - Read payout data
- `PAYOUT_WRITE` - Modify payout data
- `REPORT_CREATE` - Create reports
- `TEAM_READ` - Read team information
- `TEAM_WRITE` - Modify team membership

Full scope list: https://github.com/modrinth/labrinth/blob/master/src/models/pats.rs#L15

Requests with invalid or insufficient scopes return a `401 Unauthorized` error.

### Personal Access Tokens

Generate PATs from your user settings: https://modrinth.com/settings/account

**Recommended**: Use PATs for programmatic access and integrations.

### OAuth2

OAuth2 allows applications to request specific scopes and act on behalf of users.

**Guide**: https://docs.modrinth.com/guide/oauth/

### GitHub Tokens (Deprecated)

**DEPRECATED**: GitHub tokens currently work but will be removed in API v3. Migrate to PATs immediately.

---

## Rate Limiting

### Current Limits

- **Rate Limit**: 300 requests per minute
- **Scope**: Per IP address
- **Authentication Impact**: Rate limits are the same for authenticated and unauthenticated requests

### Rate Limit Headers

Every response includes rate limit information:

| Header | Description |
|--------|-------------|
| `X-Ratelimit-Limit` | Maximum requests per minute (currently 300) |
| `X-Ratelimit-Remaining` | Requests remaining in current window |
| `X-Ratelimit-Reset` | Seconds until rate limit window resets |

### Rate Limit Response

When rate limited, you'll receive:

```
HTTP/1.1 429 Too Many Requests
X-Ratelimit-Limit: 300
X-Ratelimit-Remaining: 0
X-Ratelimit-Reset: 42
Retry-After: 42
```

### Higher Limits

If your use case requires higher limits, contact Modrinth: admin@modrinth.com

---

## User-Agent Requirements

**CRITICAL**: A uniquely-identifying `User-Agent` header is **REQUIRED** for all requests.

### User-Agent Format

- **Bad**: `User-Agent: okhttp/4.9.3` (library-only identification)
- **Good**: `User-Agent: project_name`
- **Better**: `User-Agent: github_username/project_name/1.56.0`
- **Best**: `User-Agent: github_username/project_name/1.56.0 (launcher.com)`
- **Best**: `User-Agent: github_username/project_name/1.56.0 (contact@launcher.com)`

Generic HTTP library user agents (e.g., "okhttp", "python-requests") will likely be blocked. Include contact information so Modrinth can reach you before blocking your traffic.

---

## Cross-Origin Resource Sharing (CORS)

Modrinth implements CORS in compliance with the W3C specification. All responses have wildcard same-origin headers, making the API completely accessible from browser-based applications.

---

## Identifiers

### Base62 IDs

Most resources use unique 8-digit base62 IDs:

- Projects: `AABBCCDD`
- Versions: `IIJJKKLL`
- Users: `EEFFGGHH`
- Teams: `MMNNOOPP`
- Threads: `TTUUVVWW`
- Reports: Similar format

### Slugs and Usernames

- **Projects**: Have slugs (e.g., `fabric-api`, `sodium`)
- **Users**: Have usernames (e.g., `jellysquid`)

**Important**: Slugs and usernames can change. For long-term storage, use base62 IDs.

### File Identifiers

Version files are identified by their **SHA-1** or **SHA-512** hashes.

### Identifier Pattern

Most endpoints accept either format:
- `/project/{id|slug}` - Accepts `AABBCCDD` or `my_project`
- `/user/{id|username}` - Accepts `EEFFGGHH` or `my_username`

---

## API Versioning

### Versioning Pattern

Modrinth uses URL-based versioning. Breaking changes increment the major version:

- API v1: `/v1/...` (Deprecated)
- API v2: `/v2/...` (Current)
- API v3: `/v3/...` (Future)

### Deprecation Policy

When a new API version is released:

1. Previous version is immediately deprecated
2. No further support for older versions
3. Deprecated versions remain available temporarily
4. Deprecated endpoints may return warnings (e.g., "STOP USING THIS API")
5. Fully deprecated APIs return `410 Gone`

**Important**: Handle `410 Gone` errors gracefully and migrate promptly.

---

## Search & Filtering

### Search Endpoint

```
GET /search
```

Search for projects (mods, modpacks, resource packs, shaders).

#### Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `query` | string | - | Search query (e.g., "gravestones") |
| `facets` | string | - | Filters (see Facets section below) |
| `index` | string | `relevance` | Sort method: `relevance`, `downloads`, `follows`, `newest`, `updated` |
| `offset` | integer | `0` | Skip this many results (pagination) |
| `limit` | integer | `10` | Number of results to return (max 100) |

#### Facets (Filtering)

Facets allow complex filtering with AND/OR logic.

##### Common Facet Types

- `project_type` - `mod`, `modpack`, `resourcepack`, `shader`
- `categories` - Includes loaders (e.g., `forge`, `fabric`, `quilt`)
- `versions` - Minecraft versions (e.g., `1.19.2`, `1.20.1`)
- `client_side` - `required`, `optional`, `unsupported`
- `server_side` - `required`, `optional`, `unsupported`
- `open_source` - `true`, `false`

##### Advanced Facet Types

- `title` - Project title
- `author` - Author username
- `follows` - Number of followers
- `project_id` - Specific project ID
- `license` - License identifier (e.g., `MIT`, `GPL-3.0`)
- `downloads` - Download count
- `color` - Project color (RGB integer)
- `created_timestamp` - Unix timestamp
- `modified_timestamp` - Unix timestamp
- `date_created` - ISO-8601 timestamp
- `date_modified` - ISO-8601 timestamp

##### Facet Operators

- `:` or `=` - Equal
- `!=` - Not equal
- `>=` - Greater than or equal
- `>` - Greater than
- `<=` - Less than or equal
- `<` - Less than

##### Facet Syntax

```
{type} {operator} {value}
```

Examples:
```
categories:adventure
versions!=1.20.1
downloads<=100
```

##### Facet Logic

**OR Logic**: Elements in the same array
```json
[["versions:1.16.5", "versions:1.17.1"]]
```
Translates to: "Projects that support 1.16.5 OR 1.17.1"

**AND Logic**: Separate arrays
```json
[["versions:1.16.5"], ["project_type:modpack"]]
```
Translates to: "Projects that support 1.16.5 AND are modpacks"

**Complex Example**:
```json
[["categories:forge"],["versions:1.17.1"],["project_type:mod"],["license:mit"]]
```
Translates to: "Forge mods for 1.17.1 with MIT license"

#### Response Schema

```json
{
  "hits": [
    {
      "slug": "my_project",
      "title": "My Project",
      "description": "A short description",
      "categories": ["technology", "adventure", "fabric"],
      "client_side": "required",
      "server_side": "optional",
      "project_type": "mod",
      "downloads": 1000000,
      "icon_url": "https://cdn.modrinth.com/data/AABBCCDD/icon.png",
      "color": 8703084,
      "thread_id": "TTUUVVWW",
      "monetization_status": "monetized",
      "project_id": "AABBCCDD",
      "author": "my_user",
      "display_categories": ["technology", "fabric"],
      "versions": ["1.19", "1.19.1", "1.19.2"],
      "follows": 5000,
      "date_created": "2022-01-01T00:00:00Z",
      "date_modified": "2023-12-01T12:00:00Z",
      "latest_version": "1.19.2",
      "license": "MIT",
      "gallery": [
        "https://cdn.modrinth.com/data/AABBCCDD/images/image1.png"
      ],
      "featured_gallery": "https://cdn.modrinth.com/data/AABBCCDD/images/featured.png"
    }
  ],
  "offset": 0,
  "limit": 10,
  "total_hits": 150
}
```

#### Pagination

Use `offset` and `limit` for pagination:

```
GET /search?query=fabric&limit=20&offset=0   // Page 1
GET /search?query=fabric&limit=20&offset=20  // Page 2
GET /search?query=fabric&limit=20&offset=40  // Page 3
```

**Limit**: Maximum 100 results per request.

---

## Dependency Resolution

### Get Project Dependencies

```
GET /project/{id|slug}/dependencies
```

Retrieves all projects and versions that the specified project depends on.

#### Parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `id\|slug` | string | Project ID (base62) or slug |

#### Response Schema

```json
{
  "projects": [
    {
      "id": "P455w0rd5L1b",
      "slug": "p455w0rds-library",
      "title": "p455w0rd's Library",
      "description": "A library mod",
      "categories": ["library"],
      "client_side": "required",
      "server_side": "required",
      "project_type": "mod",
      "downloads": 500000,
      "icon_url": "https://cdn.modrinth.com/data/...",
      "color": 1234567,
      "versions": ["1.19", "1.20"],
      "loaders": ["forge", "fabric"]
    }
  ],
  "versions": [
    {
      "id": "IIJJKKLL",
      "project_id": "AABBCCDD",
      "author_id": "EEFFGGHH",
      "name": "Version 1.0.0",
      "version_number": "1.0.0",
      "changelog": "Initial release",
      "dependencies": [
        {
          "version_id": "QQRRSSTT",
          "project_id": "P455w0rd5L1b",
          "file_name": null,
          "dependency_type": "required"
        }
      ],
      "game_versions": ["1.19.2"],
      "version_type": "release",
      "loaders": ["fabric"],
      "featured": true,
      "status": "listed",
      "date_published": "2023-01-01T00:00:00Z",
      "downloads": 10000,
      "files": [
        {
          "hashes": {
            "sha1": "c84dd4b3580c02b79958a0590afd5783d80ef504",
            "sha512": "93ecf5fe02914fb53d94aa3d28c1fb562e23985f..."
          },
          "url": "https://cdn.modrinth.com/data/AABBCCDD/versions/1.0.0/mymod.jar",
          "filename": "mymod-1.0.0.jar",
          "primary": true,
          "size": 1097270,
          "file_type": null
        }
      ]
    }
  ]
}
```

### Dependency Types

Version dependencies have four types:

| Type | Description |
|------|-------------|
| `required` | Must be installed for the mod to work |
| `optional` | Enhances functionality but not required |
| `incompatible` | Cannot be used together |
| `embedded` | Bundled within the mod file |

### Version Dependency Schema

```json
{
  "version_id": "IIJJKKLL",       // Specific version (if pinned)
  "project_id": "QQRRSSTT",       // Project being depended on
  "file_name": "sodium-fabric-mc1.19-0.4.2+build.16.jar",  // External dependency filename
  "dependency_type": "required"   // required | optional | incompatible | embedded
}
```

**Note**: Either `version_id` or `project_id` may be null. For external dependencies (non-Modrinth), `file_name` is provided.

### Loader Compatibility

Versions specify supported loaders:

- `fabric` - Fabric
- `forge` - Forge (legacy)
- `neoforge` - NeoForge
- `quilt` - Quilt
- `minecraft` - Vanilla (for resource packs, data packs)

**Loader Resolution**: When resolving dependencies, filter by:
1. `game_versions` - Must match target Minecraft version
2. `loaders` - Must match target mod loader
3. `dependency_type` - Handle based on application logic

---

## Downloads & Caching

### CDN Structure

All files are served from Modrinth's CDN:

```
https://cdn.modrinth.com/data/{project_id}/versions/{version_number}/{filename}
```

Example:
```
https://cdn.modrinth.com/data/AANobbMI/versions/mc1.19-0.4.2/sodium-fabric-mc1.19-0.4.2+build.16.jar
```

### File Hashes

All version files include cryptographic hashes for verification:

```json
{
  "hashes": {
    "sha1": "c84dd4b3580c02b79958a0590afd5783d80ef504",
    "sha512": "93ecf5fe02914fb53d94aa3d28c1fb562e23985f8e4d48b9038422798618761fe208a31ca9b723667a4e05de0d91a3f86bcd8d018f6a686c39550e21b198d96f"
  }
}
```

**Required**: SHA-1 and SHA-512
**Recommendation**: Verify downloads using SHA-512

### Version File Schema

```json
{
  "hashes": {
    "sha1": "...",
    "sha512": "..."
  },
  "url": "https://cdn.modrinth.com/data/AABBCCDD/versions/1.0.0/file.jar",
  "filename": "mymod-1.0.0.jar",
  "primary": true,
  "size": 1097270,
  "file_type": null
}
```

| Field | Type | Description |
|-------|------|-------------|
| `hashes` | object | SHA-1 and SHA-512 hashes |
| `url` | string | Direct CDN download URL |
| `filename` | string | File name |
| `primary` | boolean | Whether this is the primary file (only one per version) |
| `size` | integer | File size in bytes |
| `file_type` | string\|null | File type for additional files |

#### File Types

- `required-resource-pack` - Required resource pack
- `optional-resource-pack` - Optional resource pack
- `sources-jar` - Source code JAR
- `dev-jar` - Development JAR
- `javadoc-jar` - Javadoc JAR
- `unknown` - Unknown type
- `signature` - Digital signature
- `null` - Primary mod/plugin file

### ETag Support

Modrinth supports ETags for conditional requests and efficient caching.

#### ETag Headers

Response includes:
```
ETag: "abc123def456"
```

#### Conditional Requests

Subsequent requests should include:
```
If-None-Match: "abc123def456"
```

If content unchanged, server returns:
```
HTTP/1.1 304 Not Modified
```

If content changed, server returns:
```
HTTP/1.1 200 OK
ETag: "xyz789new012"
[full content]
```

### Caching Best Practices

1. **Cache version metadata** - Store version info locally
2. **Use ETags** - For icons, images, and project metadata
3. **Verify hashes** - Always verify downloaded files using SHA-512
4. **Respect rate limits** - Cache aggressively to minimize API calls
5. **CDN URLs are stable** - Safe to cache download URLs

### Download Authentication

**Most downloads**: No authentication required
**Private versions**: Require appropriate `VERSION_READ` scope

### Redirect Handling

CDN URLs may redirect. Follow redirects (most HTTP clients handle automatically).

---

## Endpoints

### Projects

#### Get Project

```
GET /project/{id|slug}
```

Retrieve project details by ID or slug.

**Parameters**:
- `id|slug` (path, required) - Project ID or slug

**Scopes**: None (public projects), `PROJECT_READ` (draft/private projects)

**Response**: `Project` object (see Schemas section)

---

#### Get Multiple Projects

```
GET /projects?ids=["AABBCCDD","EEFFGGHH"]
```

Retrieve multiple projects in a single request.

**Parameters**:
- `ids` (query, required) - JSON array of project IDs/slugs

**Response**: Array of `Project` objects

---

#### Get Random Projects

```
GET /projects_random?count=10
```

Get random projects (for discovery features).

**Parameters**:
- `count` (query, optional) - Number of projects (default: 10)

**Response**: Array of `Project` objects

---

#### Create Project

```
POST /project
```

Create a new project.

**Scopes**: `PROJECT_CREATE`, `PROJECT_WRITE`

**Request Body**: `CreatableProject` (see Schemas)

**Response**: `Project` object

---

#### Edit Project

```
PATCH /project/{id|slug}
```

Modify project details.

**Scopes**: `PROJECT_WRITE`

**Request Body**: `EditableProject` (partial updates)

**Response**: `Project` object

---

#### Delete Project

```
DELETE /project/{id|slug}
```

Delete a project permanently.

**Scopes**: `PROJECT_DELETE`

**Response**: `204 No Content`

---

#### Bulk Edit Projects

```
PATCH /projects
```

Edit multiple projects simultaneously.

**Scopes**: `PROJECT_WRITE`

**Request Body**: `PatchProjectsBody`

**Response**: `204 No Content`

---

#### Get Project Icon

```
GET /project/{id|slug}/icon
```

Retrieve project icon image.

**Response**: Image file (PNG, JPEG, etc.)

**Headers**: Supports `If-None-Match` for ETag caching

---

#### Update Project Icon

```
PATCH /project/{id|slug}/icon
```

Upload a new project icon.

**Scopes**: `PROJECT_WRITE`

**Request Body**: Multipart form data with image file

**Response**: `204 No Content`

---

#### Delete Project Icon

```
DELETE /project/{id|slug}/icon
```

Remove project icon.

**Scopes**: `PROJECT_WRITE`

**Response**: `204 No Content`

---

#### Check Project Slug/ID Validity

```
GET /project/{id|slug}/check
```

Check if a project slug or ID is valid and available.

**Response**: `200 OK` if valid, `404 Not Found` if not

---

#### Get Project Gallery

```
GET /project/{id|slug}/gallery
```

Get all gallery images for a project.

**Response**: Array of `GalleryImage` objects

---

#### Add Gallery Image

```
POST /project/{id|slug}/gallery
```

Upload a gallery image.

**Scopes**: `PROJECT_WRITE`

**Request Body**: Multipart form data with:
- `image` - Image file
- `featured` - Boolean (featured status)
- `title` - String (optional)
- `description` - String (optional)
- `ordering` - Integer (display order)

**Response**: `GalleryImage` object

---

#### Edit Gallery Image

```
PATCH /project/{id|slug}/gallery?url={image_url}
```

Modify gallery image metadata.

**Scopes**: `PROJECT_WRITE`

**Request Body**: Partial `GalleryImage`

**Response**: `GalleryImage` object

---

#### Delete Gallery Image

```
DELETE /project/{id|slug}/gallery?url={image_url}
```

Remove a gallery image.

**Scopes**: `PROJECT_WRITE`

**Response**: `204 No Content`

---

#### Get Project Dependencies

```
GET /project/{id|slug}/dependencies
```

Get all dependencies for a project. (See Dependency Resolution section)

**Response**: `ProjectDependencyList`

---

#### Follow Project

```
POST /project/{id|slug}/follow
```

Follow a project to receive updates.

**Scopes**: `USER_WRITE`

**Response**: `204 No Content`

---

#### Unfollow Project

```
DELETE /project/{id|slug}/follow
```

Unfollow a project.

**Scopes**: `USER_WRITE`

**Response**: `204 No Content`

---

#### Get Project Schedule

```
GET /project/{id|slug}/schedule
```

Get scheduled releases for a project.

**Scopes**: `PROJECT_READ`

**Response**: `Schedule` object

---

#### Get Project Versions

```
GET /project/{id|slug}/version
```

Get all versions of a project.

**Parameters**:
- `loaders` (query, optional) - Filter by loaders (JSON array)
- `game_versions` (query, optional) - Filter by game versions (JSON array)
- `featured` (query, optional) - Filter featured versions (boolean)

**Response**: Array of `Version` objects

---

### Versions

#### Get Version

```
GET /version/{id}
```

Get version details by ID.

**Parameters**:
- `id` (path, required) - Version ID (base62)

**Response**: `Version` object

---

#### Get Project Version by Number

```
GET /project/{id|slug}/version/{id|number}
```

Get a specific version by version number or ID.

**Response**: `Version` object

---

#### Create Version

```
POST /version
```

Create a new version for a project.

**Scopes**: `VERSION_CREATE`, `VERSION_WRITE`

**Request Body**: Multipart form data with:
- `data` (JSON) - `CreatableVersion` object
- `file` - JAR/ZIP file(s)

**Response**: `Version` object

**Validation**: Files are validated for correctness (e.g., Forge mods must have `mods.toml`)

---

#### Edit Version

```
PATCH /version/{id}
```

Modify version details.

**Scopes**: `VERSION_WRITE`

**Request Body**: `EditableVersion` (partial)

**Response**: `Version` object

---

#### Delete Version

```
DELETE /version/{id}
```

Delete a version.

**Scopes**: `VERSION_DELETE`

**Response**: `204 No Content`

---

#### Get Multiple Versions

```
GET /versions?ids=["IIJJKKLL","QQRRSSTT"]
```

Retrieve multiple versions in one request.

**Parameters**:
- `ids` (query, required) - JSON array of version IDs

**Response**: Array of `Version` objects

---

#### Get Version Schedule

```
GET /version/{id}/schedule
```

Get scheduled release details.

**Scopes**: `VERSION_READ`

**Response**: `Schedule` object

---

#### Add Version File

```
POST /version/{id}/file
```

Add additional file to an existing version.

**Scopes**: `VERSION_WRITE`

**Request Body**: Multipart form data with file

**Response**: `204 No Content`

---

### Version Files

#### Get Version from File Hash

```
GET /version_file/{hash}?algorithm={sha1|sha512}
```

Retrieve version information from file hash.

**Parameters**:
- `hash` (path, required) - SHA-1 or SHA-512 hash
- `algorithm` (query, optional) - Hash algorithm (default: sha1)

**Response**: `Version` object

**Use Case**: Identify which version a local file belongs to

---

#### Get Multiple Versions from Hashes

```
POST /version_files
```

Get versions for multiple file hashes.

**Request Body**:
```json
{
  "hashes": [
    "c84dd4b3580c02b79958a0590afd5783d80ef504",
    "93ecf5fe02914fb53d94aa3d28c1fb562e23985f..."
  ],
  "algorithm": "sha1"
}
```

**Response**:
```json
{
  "c84dd4b3580c02b79958a0590afd5783d80ef504": {
    "id": "IIJJKKLL",
    "project_id": "AABBCCDD",
    ...
  }
}
```

---

#### Get Latest Version from Hash

```
POST /version_file/{hash}/update
```

Find the latest compatible version for a file hash.

**Parameters**:
- `hash` (path, required) - File hash

**Request Body**:
```json
{
  "loaders": ["fabric", "quilt"],
  "game_versions": ["1.19.2", "1.19.3"]
}
```

**Response**: `Version` object (latest matching version)

**Use Case**: Update checking for existing installations

---

#### Get Latest Versions from Multiple Hashes

```
POST /version_files/update
```

Batch update checking for multiple files.

**Request Body**:
```json
{
  "hashes": ["hash1", "hash2"],
  "algorithm": "sha512",
  "loaders": ["fabric"],
  "game_versions": ["1.19.2"]
}
```

**Response**: Hash-to-version map

---

#### Delete Version File

```
DELETE /version_file/{hash}?algorithm={sha1|sha512}&version_id={version_id}
```

Delete a file from a version.

**Scopes**: `VERSION_WRITE`

**Parameters**:
- `hash` (path, required)
- `algorithm` (query, optional)
- `version_id` (query, optional) - Required if hash appears in multiple versions

**Response**: `204 No Content`

---

### Users

#### Get User

```
GET /user/{id|username}
```

Get user information.

**Scopes**: `USER_READ` (for private data), `USER_READ_EMAIL` (for email)

**Response**: `User` object

---

#### Get Current User

```
GET /user
```

Get authenticated user's information.

**Scopes**: `USER_READ`

**Response**: `User` object

---

#### Edit User

```
PATCH /user/{id|username}
```

Modify user profile.

**Scopes**: `USER_WRITE`

**Request Body**: `EditableUser` (partial)

**Response**: `User` object

---

#### Get Multiple Users

```
GET /users?ids=["EEFFGGHH","user123"]
```

Retrieve multiple users.

**Parameters**:
- `ids` (query, required) - JSON array of user IDs/usernames

**Response**: Array of `User` objects

---

#### Get User Icon

```
GET /user/{id|username}/icon
```

Get user avatar/icon.

**Response**: Image file

**Headers**: Supports ETag caching

---

#### Update User Icon

```
PATCH /user/{id|username}/icon
```

Upload new user avatar.

**Scopes**: `USER_WRITE`

**Request Body**: Multipart form data with image

**Response**: `204 No Content`

---

#### Get User's Projects

```
GET /user/{id|username}/projects
```

Get all projects created by a user.

**Response**: Array of `Project` objects

---

#### Get User's Followed Projects

```
GET /user/{id|username}/follows
```

Get projects followed by a user.

**Scopes**: `USER_READ` (for private follows)

**Response**: Array of `Project` objects

---

#### Get User Payouts

```
GET /user/{id|username}/payouts
```

Get payout history and data.

**Scopes**: `PAYOUT_READ`

**Response**: `UserPayoutHistory`

---

### Notifications

#### Get User Notifications

```
GET /user/{id|username}/notifications
```

Get notifications for a user.

**Scopes**: `NOTIFICATION_READ`

**Response**: Array of `Notification` objects

---

#### Get Notification

```
GET /notification/{id}
```

Get a specific notification.

**Scopes**: `NOTIFICATION_READ`

**Response**: `Notification` object

---

#### Mark Notification as Read

```
PATCH /notification/{id}
```

Mark notification as read.

**Scopes**: `NOTIFICATION_WRITE`

**Response**: `204 No Content`

---

#### Mark Multiple Notifications as Read

```
PATCH /notifications
```

Batch mark notifications as read.

**Scopes**: `NOTIFICATION_WRITE`

**Request Body**:
```json
{
  "ids": ["NOTIF001", "NOTIF002"]
}
```

**Response**: `204 No Content`

---

#### Delete Notification

```
DELETE /notification/{id}
```

Delete a notification.

**Scopes**: `NOTIFICATION_WRITE`

**Response**: `204 No Content`

---

#### Delete Multiple Notifications

```
DELETE /notifications
```

Batch delete notifications.

**Scopes**: `NOTIFICATION_WRITE`

**Request Body**:
```json
{
  "ids": ["NOTIF001", "NOTIF002"]
}
```

**Response**: `204 No Content`

---

### Reports

#### Create Report

```
POST /report
```

Report a project, version, or user.

**Scopes**: `REPORT_CREATE`

**Request Body**: `CreatableReport`

**Response**: `Report` object

---

#### Get Report

```
GET /report/{id}
```

Get report details (moderators only).

**Scopes**: `REPORT_READ`

**Response**: `Report` object

---

#### Get Reports

```
GET /reports
```

Get all reports (moderators only).

**Scopes**: `REPORT_READ`

**Response**: Array of `Report` objects

---

#### Update Report

```
PATCH /report/{id}
```

Update report status (moderators only).

**Scopes**: `REPORT_WRITE`

**Response**: `Report` object

---

### Threads & Messages

#### Get Thread

```
GET /thread/{id}
```

Get thread details.

**Scopes**: `THREAD_READ`

**Response**: `Thread` object

---

#### Get Threads

```
GET /threads
```

Get threads for authenticated user.

**Scopes**: `THREAD_READ`

**Response**: Array of `Thread` objects

---

#### Send Message

```
POST /thread/{id}
```

Send a message to a thread.

**Scopes**: `THREAD_WRITE`

**Request Body**: `ThreadMessageBody`

**Response**: `ThreadMessage` object

---

#### Get Message

```
GET /message/{id}
```

Get a specific message.

**Scopes**: `THREAD_READ`

**Response**: `ThreadMessage` object

---

#### Delete Message

```
DELETE /message/{id}
```

Delete a message.

**Scopes**: `THREAD_WRITE`

**Response**: `204 No Content`

---

### Teams

#### Get Project Team Members

```
GET /project/{id|slug}/members
```

Get team members for a project.

**Response**: Array of `TeamMember` objects

---

#### Get Team Members

```
GET /team/{id}/members
```

Get team members by team ID.

**Response**: Array of `TeamMember` objects

---

#### Add Team Member

```
POST /team/{id}/members
```

Add a member to a team.

**Scopes**: `TEAM_WRITE`

**Request Body**: `ModifyTeamMemberBody`

**Response**: `204 No Content`

---

#### Get Multiple Teams

```
GET /teams?ids=["MMNNOOPP","QQRRSSTT"]
```

Get multiple teams.

**Parameters**:
- `ids` (query, required) - JSON array of team IDs

**Response**: Array of `Team` objects

---

#### Join Team

```
POST /team/{id}/join
```

Accept team invitation.

**Scopes**: `TEAM_WRITE`

**Response**: `204 No Content`

---

#### Edit Team Member

```
PATCH /team/{id}/members/{id|username}
```

Modify team member role/permissions.

**Scopes**: `TEAM_WRITE`

**Request Body**: `ModifyTeamMemberBody`

**Response**: `204 No Content`

---

#### Remove Team Member

```
DELETE /team/{id}/members/{id|username}
```

Remove member from team.

**Scopes**: `TEAM_WRITE`

**Response**: `204 No Content`

---

#### Transfer Team Ownership

```
PATCH /team/{id}/owner
```

Transfer team ownership to another member.

**Scopes**: `TEAM_WRITE`

**Request Body**:
```json
{
  "user_id": "EEFFGGHH"
}
```

**Response**: `204 No Content`

---

### Tags

Tags are metadata categories used for filtering and organization.

#### Get Categories

```
GET /tag/category
```

Get all available categories (e.g., "technology", "adventure").

**Response**: Array of `CategoryTag` objects

---

#### Get Loaders

```
GET /tag/loader
```

Get all supported loaders (e.g., "fabric", "forge", "quilt").

**Response**: Array of `LoaderTag` objects

---

#### Get Game Versions

```
GET /tag/game_version
```

Get all Minecraft versions (e.g., "1.19.2", "1.20.1").

**Response**: Array of `GameVersionTag` objects

---

#### Get Licenses

```
GET /tag/license
```

Get all available licenses.

**Response**: Array of `LicenseTag` objects

---

#### Get License by ID

```
GET /tag/license/{id}
```

Get details for a specific license.

**Parameters**:
- `id` (path, required) - License ID (e.g., "MIT", "GPL-3.0")

**Response**: `License` object

---

#### Get Donation Platforms

```
GET /tag/donation_platform
```

Get supported donation platforms.

**Response**: Array of `DonationPlatformTag` objects

---

#### Get Report Types

```
GET /tag/report_type
```

Get available report types.

**Response**: Array of report types

---

#### Get Project Types

```
GET /tag/project_type
```

Get project types (mod, modpack, resourcepack, shader).

**Response**: Array of project types

---

#### Get Side Types

```
GET /tag/side_type
```

Get side types (client, server).

**Response**: Array of side types

---

### Legacy & Special

#### Forge Update Checker

```
GET /updates/{id|slug}/forge_updates.json
```

Forge-compatible update checker endpoint.

**Response**: `ForgeUpdates` object

---

#### Statistics

```
GET /statistics
```

Get platform-wide statistics.

**Response**: `Statistics` object

```json
{
  "projects": 50000,
  "versions": 200000,
  "files": 300000,
  "authors": 10000
}
```

---

## Response Schemas

### Project Schema

```typescript
{
  // Identity
  id: string,                    // Base62 ID (e.g., "AABBCCDD")
  slug: string,                  // URL slug (e.g., "fabric-api")
  title: string,                 // Display name
  description: string,           // Short description

  // Classification
  project_type: "mod" | "modpack" | "resourcepack" | "shader",
  categories: string[],          // Tags (includes loaders)
  additional_categories: string[], // Secondary tags

  // Support
  client_side: "required" | "optional" | "unsupported" | "unknown",
  server_side: "required" | "optional" | "unsupported" | "unknown",

  // Content
  body: string,                  // Long description (Markdown)
  body_url: null,                // Deprecated, always null

  // Media
  icon_url: string | null,       // Icon URL
  color: number | null,          // RGB color integer
  gallery: GalleryImage[],       // Gallery images

  // Metadata
  published: string,             // ISO-8601 timestamp
  updated: string,               // ISO-8601 timestamp
  approved: string | null,       // ISO-8601 timestamp
  queued: string | null,         // ISO-8601 timestamp

  // Status
  status: "approved" | "archived" | "rejected" | "draft" | "unlisted" |
          "processing" | "withheld" | "scheduled" | "private" | "unknown",
  requested_status: "approved" | "archived" | "unlisted" | "private" | "draft" | null,
  moderator_message: ModeratorMessage | null,

  // Social
  downloads: number,             // Total downloads
  followers: number,             // Follower count

  // Links
  issues_url: string | null,
  source_url: string | null,
  wiki_url: string | null,
  discord_url: string | null,
  donation_urls: DonationURL[],

  // License
  license: {
    id: string,                  // SPDX ID (e.g., "MIT")
    name: string,                // Full name
    url: string | null           // License URL
  },

  // Versions & Compatibility
  versions: string[],            // Version IDs
  game_versions: string[],       // Minecraft versions
  loaders: string[],             // Mod loaders

  // Team
  team: string,                  // Team ID

  // Moderation
  thread_id: string,             // Moderation thread ID
  monetization_status: "monetized" | "demonetized" | "force-demonetized"
}
```

### Version Schema

```typescript
{
  // Identity
  id: string,                    // Base62 ID
  project_id: string,            // Parent project ID
  author_id: string,             // Author user ID

  // Version Info
  name: string,                  // Display name (e.g., "Version 1.0.0")
  version_number: string,        // Semantic version (e.g., "1.0.0")
  changelog: string | null,      // Markdown changelog
  changelog_url: null,           // Deprecated, always null

  // Type & Status
  version_type: "release" | "beta" | "alpha",
  status: "listed" | "archived" | "draft" | "unlisted" | "scheduled" | "unknown",
  requested_status: "listed" | "archived" | "draft" | "unlisted" | null,

  // Compatibility
  game_versions: string[],       // Minecraft versions
  loaders: string[],             // Mod loaders

  // Dependencies
  dependencies: VersionDependency[],

  // Files
  files: VersionFile[],

  // Metadata
  date_published: string,        // ISO-8601
  downloads: number,
  featured: boolean              // Featured status
}
```

### VersionDependency Schema

```typescript
{
  version_id: string | null,     // Specific version (if pinned)
  project_id: string | null,     // Project ID
  file_name: string | null,      // External dependency filename
  dependency_type: "required" | "optional" | "incompatible" | "embedded"
}
```

### VersionFile Schema

```typescript
{
  hashes: {
    sha1: string,                // SHA-1 hash
    sha512: string               // SHA-512 hash
  },
  url: string,                   // CDN download URL
  filename: string,              // File name
  primary: boolean,              // Primary file flag
  size: number,                  // Bytes
  file_type: "required-resource-pack" | "optional-resource-pack" |
             "sources-jar" | "dev-jar" | "javadoc-jar" |
             "unknown" | "signature" | null
}
```

### User Schema

```typescript
{
  // Identity
  id: string,                    // Base62 ID
  username: string,              // Username
  name: string | null,           // Display name

  // Profile
  email: string | null,          // Email (requires USER_READ_EMAIL scope)
  bio: string | null,            // Biography
  avatar_url: string | null,     // Avatar URL

  // Timestamps
  created: string,               // ISO-8601

  // Role
  role: "admin" | "moderator" | "developer" | "user",

  // Badges (array of badge IDs)
  badges: number
}
```

### GalleryImage Schema

```typescript
{
  url: string,                   // Image URL
  featured: boolean,             // Featured flag
  title: string | null,          // Image title
  description: string | null,    // Image description
  created: string,               // ISO-8601
  ordering: number               // Display order (lower = earlier)
}
```

### Notification Schema

```typescript
{
  id: string,                    // Notification ID
  user_id: string,               // Recipient user ID
  type: string,                  // Notification type (e.g., "project_update")
  title: string,                 // Notification title
  text: string,                  // Notification body
  link: string,                  // Related URL
  read: boolean,                 // Read status
  created: string,               // ISO-8601
  actions: NotificationAction[]  // Available actions
}
```

### TeamMember Schema

```typescript
{
  team_id: string,               // Team ID
  user: User,                    // User object
  role: string,                  // Role name
  permissions: number,           // Permission bitfield
  accepted: boolean,             // Invitation accepted
  ordering: number               // Display order
}
```

### SearchResults Schema

```typescript
{
  hits: ProjectResult[],         // Search results
  offset: number,                // Results skipped
  limit: number,                 // Results returned
  total_hits: number             // Total matching results
}
```

---

## Error Handling

### Error Response Schema

```typescript
{
  error: string,                 // Error name
  description: string            // Error details
}
```

### HTTP Status Codes

| Code | Meaning | Description |
|------|---------|-------------|
| 200 | OK | Request successful |
| 204 | No Content | Success, no response body |
| 304 | Not Modified | Resource unchanged (ETag match) |
| 400 | Bad Request | Invalid input or malformed request |
| 401 | Unauthorized | Missing, invalid, or insufficient token scopes |
| 403 | Forbidden | Action not permitted |
| 404 | Not Found | Resource not found or no access |
| 410 | Gone | API version deprecated |
| 429 | Too Many Requests | Rate limit exceeded |
| 500 | Internal Server Error | Server error |

### Common Errors

#### Invalid Input (400)

```json
{
  "error": "invalid_input",
  "description": "Error while parsing multipart payload"
}
```

**Causes**:
- Malformed JSON
- Invalid facet syntax
- Missing required fields
- Invalid enum values

#### Unauthorized (401)

```json
{
  "error": "unauthorized",
  "description": "Authentication required"
}
```

**Causes**:
- Missing Authorization header
- Invalid token
- Insufficient scopes

#### Rate Limited (429)

```
HTTP/1.1 429 Too Many Requests
X-Ratelimit-Limit: 300
X-Ratelimit-Remaining: 0
X-Ratelimit-Reset: 42
Retry-After: 42
```

**Response**:
- Wait for `X-Ratelimit-Reset` seconds
- Respect `Retry-After` header
- Implement exponential backoff

#### Not Found (404)

```json
{
  "error": "not_found",
  "description": "The requested item(s) were not found or no authorization to access the requested item(s)"
}
```

**Causes**:
- Invalid ID/slug
- Private resource without proper scopes
- Deleted resource

#### Gone (410)

```json
{
  "error": "gone",
  "description": "This API version is deprecated and no longer available"
}
```

**Action**: Migrate to current API version immediately

---

## Best Practices

### Rate Limit Compliance

1. **Monitor headers** - Track `X-Ratelimit-Remaining`
2. **Implement backoff** - Exponential backoff on 429 errors
3. **Cache aggressively** - Cache search results, project metadata
4. **Batch requests** - Use bulk endpoints (`/projects`, `/versions`, `/version_files`)
5. **Use ETags** - Reduce bandwidth with conditional requests

### Caching Strategy

#### What to Cache

- **Project metadata** - TTL: 5-15 minutes
- **Version lists** - TTL: 5-15 minutes
- **Search results** - TTL: 5-10 minutes
- **Tag lists** - TTL: 1 hour (changes infrequently)
- **User profiles** - TTL: 10 minutes
- **Icons/images** - Use ETags, cache indefinitely

#### Cache Invalidation

- On project updates: Invalidate project cache
- On version creation: Invalidate version list cache
- Use `updated` timestamp to detect stale cache

### Pagination

For large result sets:

1. **Start small** - Use `limit=20` initially
2. **Increase gradually** - Up to `limit=100` if needed
3. **Track offset** - `offset = page * limit`
4. **Monitor total_hits** - Determine total pages

### Dependency Resolution

1. **Fetch project dependencies** - `GET /project/{id}/dependencies`
2. **Filter by loader and game version**
3. **Resolve transitive dependencies** - Recursively fetch dependencies
4. **Handle conflicts** - Check `incompatible` dependencies
5. **Respect dependency types**:
   - `required`: Must install
   - `optional`: User choice
   - `incompatible`: Must not install together
   - `embedded`: Already included

### File Verification

Always verify downloads:

```python
import hashlib

def verify_file(file_path, expected_sha512):
    sha512 = hashlib.sha512()
    with open(file_path, 'rb') as f:
        for chunk in iter(lambda: f.read(4096), b""):
            sha512.update(chunk)
    return sha512.hexdigest() == expected_sha512
```

### Error Handling

```python
import time

def api_request_with_retry(url, max_retries=3):
    for attempt in range(max_retries):
        response = requests.get(url)

        if response.status_code == 200:
            return response.json()

        elif response.status_code == 429:
            # Rate limited
            retry_after = int(response.headers.get('Retry-After', 60))
            time.sleep(retry_after)

        elif response.status_code == 404:
            # Not found
            return None

        elif response.status_code >= 500:
            # Server error, retry with backoff
            time.sleep(2 ** attempt)

        else:
            # Other error, don't retry
            raise Exception(f"API error: {response.status_code}")

    raise Exception("Max retries exceeded")
```

### User-Agent Implementation

```python
import requests

APP_NAME = "MyLauncher"
APP_VERSION = "1.0.0"
CONTACT = "admin@mylauncher.com"

headers = {
    "User-Agent": f"{APP_NAME}/{APP_VERSION} ({CONTACT})"
}

response = requests.get("https://api.modrinth.com/v2/search", headers=headers)
```

---

## API v1 to v2 Migration

### Key Changes

1. **Search endpoint moved**: `/api/v1/mod` → `/v2/search`
2. **Field renames**: `mod_id` → `project_id`, `mod_*` → `project_*`
3. **New project field**: `project_type` (mod, modpack, resourcepack, shader)
4. **New search facet**: `project_type`
5. **Alphabetical sort removed** (was broken)
6. **Gallery feature added**: Projects can have multiple gallery images
7. **File validation**: Uploaded files are validated (e.g., Forge mods need `mods.toml`)
8. **Project status**: New `archived` status (excluded from search)
9. **Tag icons**: Tags now have SVG icons and project type associations
10. **Dependencies redesigned**: New dependency system with types
11. **Notification types**: Notifications now have `type` field
12. **Slugs everywhere**: Endpoints accept slugs in addition to IDs
13. **Donation URLs enabled**

### Deprecated Features

- `body_url` - Always null
- `changelog_url` - Always null
- GitHub tokens - Will be removed in API v3

---

## Changelog & Updates

### Version History

- **v2.7.0/366f528** (Current) - Latest stable release
- **v2.x** - Active development
- **v1.x** - Deprecated

### Upcoming Changes (API v3)

- GitHub token authentication will be removed
- Potential breaking changes TBD

### Staying Updated

- **Changelog**: https://modrinth.com/news/changelog
- **Documentation**: https://docs.modrinth.com
- **API Spec**: https://docs.modrinth.com/openapi.yaml

---

## Support & Resources

### Official Resources

- **Documentation**: https://docs.modrinth.com
- **API Specification**: https://docs.modrinth.com/openapi.yaml
- **Support Center**: https://support.modrinth.com
- **Contact**: support@modrinth.com
- **Terms of Service**: https://modrinth.com/legal/terms

### Community Resources

- **GitHub**: https://github.com/modrinth
- **Labrinth (API backend)**: https://github.com/modrinth/labrinth
- **Issue Tracker**: https://github.com/modrinth/docs/issues

### Client Libraries

Multiple community-maintained client libraries are available:

- **Rust**: `modrinth-api` - https://crates.io/crates/modrinth-api
- **PHP**: `aternos/modrinth-api` - https://github.com/aternosorg/php-modrinth-api
- **Python**: `modrinth` - https://pypi.org/project/modrinth/
- **Dart**: `modrinth_api` - https://pub.dev/packages/modrinth_api

All libraries are generated from the OpenAPI specification.

---

## Appendix: Complete Endpoint List

| Method | Endpoint | Description | Auth Required |
|--------|----------|-------------|---------------|
| GET | `/` | API information | No |
| GET | `/search` | Search projects | No |
| GET | `/project/{id\|slug}` | Get project | Conditional |
| GET | `/projects` | Get multiple projects | Conditional |
| GET | `/projects_random` | Get random projects | No |
| POST | `/project` | Create project | Yes |
| PATCH | `/project/{id\|slug}` | Edit project | Yes |
| DELETE | `/project/{id\|slug}` | Delete project | Yes |
| PATCH | `/projects` | Bulk edit projects | Yes |
| GET | `/project/{id\|slug}/icon` | Get project icon | No |
| PATCH | `/project/{id\|slug}/icon` | Update project icon | Yes |
| DELETE | `/project/{id\|slug}/icon` | Delete project icon | Yes |
| GET | `/project/{id\|slug}/check` | Check project validity | No |
| GET | `/project/{id\|slug}/gallery` | Get gallery images | No |
| POST | `/project/{id\|slug}/gallery` | Add gallery image | Yes |
| PATCH | `/project/{id\|slug}/gallery` | Edit gallery image | Yes |
| DELETE | `/project/{id\|slug}/gallery` | Delete gallery image | Yes |
| GET | `/project/{id\|slug}/dependencies` | Get dependencies | No |
| POST | `/project/{id\|slug}/follow` | Follow project | Yes |
| DELETE | `/project/{id\|slug}/follow` | Unfollow project | Yes |
| GET | `/project/{id\|slug}/schedule` | Get project schedule | Yes |
| GET | `/project/{id\|slug}/version` | Get project versions | No |
| GET | `/version/{id}` | Get version | Conditional |
| GET | `/project/{id\|slug}/version/{id\|number}` | Get specific version | Conditional |
| POST | `/version` | Create version | Yes |
| PATCH | `/version/{id}` | Edit version | Yes |
| DELETE | `/version/{id}` | Delete version | Yes |
| GET | `/versions` | Get multiple versions | Conditional |
| GET | `/version/{id}/schedule` | Get version schedule | Yes |
| POST | `/version/{id}/file` | Add version file | Yes |
| GET | `/version_file/{hash}` | Get version from hash | No |
| DELETE | `/version_file/{hash}` | Delete version file | Yes |
| POST | `/version_files` | Get versions from hashes | No |
| POST | `/version_file/{hash}/update` | Get latest version from hash | No |
| POST | `/version_files/update` | Get latest versions from hashes | No |
| GET | `/user/{id\|username}` | Get user | Conditional |
| PATCH | `/user/{id\|username}` | Edit user | Yes |
| GET | `/user` | Get current user | Yes |
| GET | `/users` | Get multiple users | Conditional |
| GET | `/user/{id\|username}/icon` | Get user icon | No |
| PATCH | `/user/{id\|username}/icon` | Update user icon | Yes |
| GET | `/user/{id\|username}/projects` | Get user's projects | No |
| GET | `/user/{id\|username}/follows` | Get followed projects | Conditional |
| GET | `/user/{id\|username}/payouts` | Get user payouts | Yes |
| GET | `/user/{id\|username}/notifications` | Get notifications | Yes |
| GET | `/notification/{id}` | Get notification | Yes |
| PATCH | `/notification/{id}` | Mark notification read | Yes |
| DELETE | `/notification/{id}` | Delete notification | Yes |
| PATCH | `/notifications` | Mark notifications read | Yes |
| DELETE | `/notifications` | Delete notifications | Yes |
| POST | `/report` | Create report | Yes |
| GET | `/report/{id}` | Get report | Yes |
| PATCH | `/report/{id}` | Update report | Yes |
| GET | `/reports` | Get reports | Yes |
| GET | `/thread/{id}` | Get thread | Yes |
| GET | `/threads` | Get threads | Yes |
| POST | `/thread/{id}` | Send message | Yes |
| GET | `/message/{id}` | Get message | Yes |
| DELETE | `/message/{id}` | Delete message | Yes |
| GET | `/project/{id\|slug}/members` | Get project team | No |
| GET | `/team/{id}/members` | Get team members | No |
| POST | `/team/{id}/members` | Add team member | Yes |
| GET | `/teams` | Get multiple teams | No |
| POST | `/team/{id}/join` | Join team | Yes |
| PATCH | `/team/{id}/members/{id\|username}` | Edit team member | Yes |
| DELETE | `/team/{id}/members/{id\|username}` | Remove team member | Yes |
| PATCH | `/team/{id}/owner` | Transfer ownership | Yes |
| GET | `/tag/category` | Get categories | No |
| GET | `/tag/loader` | Get loaders | No |
| GET | `/tag/game_version` | Get game versions | No |
| GET | `/tag/license` | Get licenses | No |
| GET | `/tag/license/{id}` | Get license details | No |
| GET | `/tag/donation_platform` | Get donation platforms | No |
| GET | `/tag/report_type` | Get report types | No |
| GET | `/tag/project_type` | Get project types | No |
| GET | `/tag/side_type` | Get side types | No |
| GET | `/updates/{id\|slug}/forge_updates.json` | Forge update checker | No |
| GET | `/statistics` | Get statistics | No |

**Total Endpoints**: 55

---

**Document Version**: 1.0
**Last Updated**: 2025-12-21
**API Version**: v2.7.0/366f528
**Compiled From**: Official Modrinth OpenAPI specification and documentation
