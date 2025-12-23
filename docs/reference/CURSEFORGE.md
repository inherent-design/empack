# CurseForge API Reference

Complete API specification for CurseForge for Studios platform. This document covers all endpoints, parameters, response schemas, and operational constraints.

## Overview

**Base URL**: `https://api.curseforge.com`

**API Version**: v1 (primary), v2 (selective endpoints)

**Platform**: CurseForge for Studios - User-generated content platform for games (mods, add-ons, modpacks)

**Content Delivery**: edge.forgecdn.net CDN for file downloads

**Protocol**: HTTPS only

**Pagination Model**: Offset-based with hard limit at 10,000 results

**Response Format**: JSON

**Common Games**: Minecraft (gameId: 432), World of Warcraft, and other moddable games

## Authentication

### API Key Generation

1. Create account at https://console.curseforge.com
2. Navigate to API Keys section in developer console
3. Generate unique API key (non-transferable)
4. Accept CurseForge 3rd Party API Terms and Conditions

### Request Authentication

**Header**: `x-api-key`

**Type**: API Key authentication

**Location**: Header parameter

**Required**: Yes, on all endpoints

**Format**:
```http
x-api-key: YOUR_API_KEY_HERE
```

### API Key Security

- API keys are unique per developer/organization
- Non-transferable - may not be shared with third parties
- Employees subject to confidentiality obligations may access keys
- API keys grant access only to games/projects authorized for that key
- Private games accessible only via their respective API keys

### API Key Acquisition

**Portal**: https://console.curseforge.com

**Process**:
1. Submit application form
2. Accept 3rd Party API Terms of Service
3. Await Overwolf review and approval
4. Receive API key via email
5. Key immediately active upon receipt

## Rate Limiting

### Free Tier Limits

**Documented Limits**: Not publicly disclosed with specific numeric values

**Observed Behavior**:
- Rate limits described by users as "annoyingly low" for free tier
- Limits enforced per API key
- Both per-endpoint and global rate limiting applied

### Rate Limit Responses

**HTTP Status**: 403 Forbidden

**Error Message**: "Access to https://api.curseforge.com is forbidden or rate-limit has been exceeded"

**Behavior**: Requests blocked until rate limit window resets

### Rate Limit Headers

**Not Documented**: CurseForge does not expose standard rate limit headers (X-RateLimit-Limit, X-RateLimit-Remaining, X-RateLimit-Reset)

**Retry-After**: Not consistently provided in 429 or 403 responses

### Rate Limit Strategy

**Detection**: Monitor for 403 responses with rate limit messaging

**Backoff**: Implement exponential backoff starting at 60 seconds

**Reset Timing**: Unknown - rate limit windows not documented

**Mitigation**:
- Cache responses aggressively (see Caching section)
- Batch requests using bulk endpoints (POST /v1/mods, POST /v1/mods/files)
- Minimize search queries - use specific mod/file IDs when possible

### Quota Tiers

**Free Tier**: API key with undisclosed rate limits

**Paid Tier**: Upon exceeding quota, may require written licensing agreement with payment requirements

**Quota Enforcement**: Overwolf may decline continued access without mutual payment agreement

## Pagination

### Pagination Parameters

**index** (integer, optional)
- Zero-based index of first item to include in response
- Default: 0
- Constraint: (index + pageSize) <= 10,000

**pageSize** (integer, optional)
- Number of items per page
- Default: 50
- Maximum: 50
- Constraint: (index + pageSize) <= 10,000

### Pagination Limits

**Hard Cap**: 10,000 total results

**Formula**: index + pageSize <= 10,000

**Examples**:
- pageSize=50, index=0: Retrieves items 0-49 (valid)
- pageSize=50, index=9950: Retrieves items 9950-9999 (valid, last page)
- pageSize=50, index=9951: Invalid (10,001 > 10,000)
- pageSize=25, index=9980: Retrieves items 9980-9999 (valid, partial last page)

**Beyond 10K**: No access to results past 10,000th item regardless of parameters

### Pagination Response

**Response Object**:
```json
{
  "pagination": {
    "index": 0,
    "pageSize": 50,
    "resultCount": 50,
    "totalCount": 1523
  }
}
```

**Fields**:
- `index`: Current page starting index (echoes request)
- `pageSize`: Items per page (echoes request)
- `resultCount`: Actual items returned on this page
- `totalCount`: Total items matching query (may exceed 10,000 but only first 10K accessible)

## Endpoints

### Games

#### GET /v1/games

Get all games available to the provided API key.

**Authentication**: Required

**Parameters**:
- `index` (integer, optional): Zero-based index for pagination
- `pageSize` (integer, optional): Items per page (default/max: 50)

**Response**: 200 OK
```json
{
  "data": [
    {
      "id": 432,
      "name": "Minecraft",
      "slug": "minecraft",
      "dateModified": "2024-01-15T10:30:00Z",
      "assets": {
        "iconUrl": "https://media.forgecdn.net/...",
        "tileUrl": "https://media.forgecdn.net/...",
        "coverUrl": "https://media.forgecdn.net/..."
      },
      "status": 2,
      "apiStatus": 2
    }
  ],
  "pagination": {
    "index": 0,
    "pageSize": 50,
    "resultCount": 12,
    "totalCount": 12
  }
}
```

**Status Codes**:
- 200: Success
- 500: Internal server error

#### GET /v1/games/{gameId}

Get a single game by ID. Private games only accessible via their respective API keys.

**Authentication**: Required

**Path Parameters**:
- `gameId` (integer, required): Game unique identifier (e.g., 432 for Minecraft)

**Response**: 200 OK
```json
{
  "data": {
    "id": 432,
    "name": "Minecraft",
    "slug": "minecraft",
    "dateModified": "2024-01-15T10:30:00Z",
    "assets": {
      "iconUrl": "https://media.forgecdn.net/...",
      "tileUrl": "https://media.forgecdn.net/...",
      "coverUrl": "https://media.forgecdn.net/..."
    },
    "status": 2,
    "apiStatus": 2
  }
}
```

**Status Codes**:
- 200: Success
- 404: Game not found or not accessible with this API key
- 500: Internal server error

#### GET /v1/games/{gameId}/versions

Get all available versions for each known version type of the specified game.

**Authentication**: Required

**Path Parameters**:
- `gameId` (integer, required): Game unique identifier

**Response**: 200 OK (v1)
```json
{
  "data": [
    {
      "type": 1,
      "versions": [
        "1.20.4",
        "1.20.3",
        "1.20.2",
        "1.20.1"
      ]
    }
  ]
}
```

**Status Codes**:
- 200: Success
- 404: Game not found
- 500: Internal server error

**Note**: v1 endpoint returns version strings as array of strings

#### GET /v2/games/{gameId}/versions

Version 2 endpoint returns structured version objects instead of strings.

**Authentication**: Required

**Path Parameters**:
- `gameId` (integer, required): Game unique identifier

**Response**: 200 OK (v2)
```json
{
  "data": [
    {
      "type": 1,
      "versions": [
        {
          "id": 9990,
          "slug": "1-20-4",
          "name": "1.20.4"
        },
        {
          "id": 9925,
          "slug": "1-20-3",
          "name": "1.20.3"
        }
      ]
    }
  ]
}
```

**Status Codes**:
- 200: Success
- 404: Game not found
- 500: Internal server error

#### GET /v1/games/{gameId}/version-types

Get all available version types for a game.

**Authentication**: Required

**Path Parameters**:
- `gameId` (integer, required): Game unique identifier

**Response**: 200 OK
```json
{
  "data": [
    {
      "id": 1,
      "gameId": 432,
      "name": "Minecraft",
      "slug": "minecraft",
      "isSyncable": true,
      "status": 2
    }
  ]
}
```

**Usage Note**: Most games created via CurseForge for Studios Console are limited to single version type. Multiple version types relevant primarily for legacy games (e.g., WoW retail vs classic).

**Status Codes**:
- 200: Success
- 404: Game not found
- 500: Internal server error

### Categories

#### GET /v1/categories

Get all classes and categories for a game, or categories under a specific class.

**Authentication**: Required

**Parameters**:
- `gameId` (integer, required): Game unique identifier
- `classId` (integer, optional): Filter categories under this class
- `classesOnly` (boolean, optional): Return only classes (no categories)

**Response**: 200 OK
```json
{
  "data": [
    {
      "id": 6,
      "gameId": 432,
      "name": "Mods",
      "slug": "mods",
      "url": "https://www.curseforge.com/minecraft/mc-mods",
      "iconUrl": "https://media.forgecdn.net/...",
      "dateModified": "2023-12-01T08:00:00Z",
      "isClass": true,
      "classId": null,
      "parentCategoryId": null,
      "displayIndex": 0
    },
    {
      "id": 423,
      "gameId": 432,
      "name": "Map and Information",
      "slug": "map-information",
      "url": "https://www.curseforge.com/minecraft/mc-mods/map-information",
      "iconUrl": "https://media.forgecdn.net/...",
      "dateModified": "2023-12-01T08:00:00Z",
      "isClass": false,
      "classId": 6,
      "parentCategoryId": 6,
      "displayIndex": 10
    }
  ]
}
```

**Category Hierarchy**:
- Classes: Top-level categories (isClass=true) - e.g., "Mods", "Modpacks", "Resource Packs"
- Categories: Subcategories under classes - e.g., "Technology", "Magic", "World Gen"
- Minecraft classId 6 = "Mods"
- Categories have parentCategoryId pointing to class

**Common Minecraft Category IDs**:
- 423: Map and Information
- 424: Cosmetic

**Status Codes**:
- 200: Success
- 404: Game not found
- 500: Internal server error

### Mods

#### GET /v1/mods/search

Search for mods matching criteria. Primary search endpoint with extensive filtering.

**Authentication**: Required

**Parameters**:

**Required**:
- `gameId` (integer): Filter by game (e.g., 432 for Minecraft)

**Filtering**:
- `classId` (integer): Section/class filter (discoverable via /v1/categories)
- `categoryId` (integer): Single category filter
- `categoryIds` (string): Comma-separated category IDs (overrides categoryId, max 10)
  - Format: `categoryIds=[1,2,3]`
- `gameVersion` (string): Filter by game version string (e.g., "1.20.4")
- `gameVersions` (string): Array of game versions (overrides gameVersion, max 4)
  - Format: `gameVersions=["1.20.4","1.20.3","1.20.2"]`
- `modLoaderType` (ModLoaderType): Filter by mod loader (requires gameVersion)
  - Enum: 0 (Any), 1 (Forge), 2 (Cauldron), 3 (LiteLoader), 4 (Fabric), 5 (Quilt), 6 (NeoForge)
- `modLoaderTypes` (string): Array of mod loader types (overrides modLoaderType, max 5)
  - Format: `modLoaderTypes=[1,4,5]`
- `gameVersionTypeId` (integer): Filter files tagged with specific version type
- `authorId` (integer): Filter mods where authorId is a member
- `primaryAuthorId` (integer): Filter mods owned by primaryAuthorId
- `slug` (string): Filter by slug (unique when combined with classId)

**Search**:
- `searchFilter` (string): Free text search in mod name and author

**Sorting**:
- `sortField` (ModsSearchSortField): Sort criteria
  - 1: Featured
  - 2: Popularity
  - 3: Last Updated
  - 4: Name
  - 5: Author
  - 6: Total Downloads
  - 7: Category
  - 8: Game Version
  - 9: Early Access (descending order only)
  - 10: Featured + Weighted (descending order only)
  - 11: Relative Popularity (descending order only)
  - 12: Fingerprint Match Exact (descending order only)
- `sortOrder` (SortOrder): asc or desc

**Pagination**:
- `index` (integer): Zero-based index (constraint: index + pageSize <= 10,000)
- `pageSize` (integer): Results per page (default/max: 50)

**Response**: 200 OK
```json
{
  "data": [
    {
      "id": 238222,
      "gameId": 432,
      "name": "Just Enough Items (JEI)",
      "slug": "jei",
      "links": {
        "websiteUrl": "https://www.curseforge.com/minecraft/mc-mods/jei",
        "wikiUrl": null,
        "issuesUrl": "https://github.com/mezz/JustEnoughItems/issues",
        "sourceUrl": "https://github.com/mezz/JustEnoughItems"
      },
      "summary": "JEI is an item and recipe viewing mod for Minecraft",
      "status": 6,
      "downloadCount": 425789012,
      "isFeatured": true,
      "primaryCategoryId": 423,
      "categories": [
        {
          "id": 423,
          "gameId": 432,
          "name": "Map and Information",
          "slug": "map-information",
          "url": "https://www.curseforge.com/minecraft/mc-mods/map-information",
          "iconUrl": "https://media.forgecdn.net/...",
          "dateModified": "2023-12-01T08:00:00Z",
          "isClass": false,
          "classId": 6,
          "parentCategoryId": 6,
          "displayIndex": 10
        }
      ],
      "classId": 6,
      "authors": [
        {
          "id": 166630,
          "name": "mezz",
          "url": "https://www.curseforge.com/members/mezz"
        }
      ],
      "logo": {
        "id": 382635,
        "modId": 238222,
        "title": "JEI Logo",
        "description": "",
        "thumbnailUrl": "https://media.forgecdn.net/.../thumbnail.png",
        "url": "https://media.forgecdn.net/.../full.png"
      },
      "screenshots": [],
      "mainFileId": 5284115,
      "latestFiles": [
        {
          "id": 5284115,
          "gameId": 432,
          "modId": 238222,
          "isAvailable": true,
          "displayName": "jei-1.20.4-17.0.0.60.jar",
          "fileName": "jei-1.20.4-17.0.0.60.jar",
          "releaseType": 1,
          "fileStatus": 4,
          "hashes": [
            {
              "value": "a3f8b7c2d1e5f6a9b8c7d6e5f4a3b2c1",
              "algo": 2
            }
          ],
          "fileDate": "2024-01-10T15:30:00Z",
          "fileLength": 1248576,
          "downloadCount": 12456,
          "fileSizeOnDisk": 1248576,
          "downloadUrl": "https://edge.forgecdn.net/files/5284/115/jei-1.20.4-17.0.0.60.jar",
          "gameVersions": [
            "1.20.4",
            "Forge"
          ],
          "sortableGameVersions": [
            {
              "gameVersionName": "1.20.4",
              "gameVersionPadded": "0000000001.0000000020.0000000004",
              "gameVersion": "1.20.4",
              "gameVersionReleaseDate": "2023-12-07T14:00:00Z",
              "gameVersionTypeId": 1
            }
          ],
          "dependencies": [
            {
              "modId": 419699,
              "relationType": 3
            }
          ],
          "exposeAsAlternative": false,
          "parentProjectFileId": null,
          "alternateFileId": null,
          "isServerPack": false,
          "serverPackFileId": null,
          "isEarlyAccessContent": false,
          "earlyAccessEndDate": null,
          "fileFingerprint": 2841256732,
          "modules": [
            {
              "name": "META-INF",
              "fingerprint": 3052645487
            },
            {
              "name": "mezz",
              "fingerprint": 2156847392
            }
          ]
        }
      ],
      "latestFilesIndexes": [
        {
          "gameVersion": "1.20.4",
          "fileId": 5284115,
          "filename": "jei-1.20.4-17.0.0.60.jar",
          "releaseType": 1,
          "gameVersionTypeId": 1,
          "modLoader": 1
        }
      ],
      "latestEarlyAccessFilesIndexes": [],
      "dateCreated": "2016-05-15T10:00:00Z",
      "dateModified": "2024-01-10T15:30:00Z",
      "dateReleased": "2024-01-10T15:30:00Z",
      "allowModDistribution": true,
      "gamePopularityRank": 1,
      "isAvailable": true,
      "thumbsUpCount": 8542,
      "rating": 4.8
    }
  ],
  "pagination": {
    "index": 0,
    "pageSize": 50,
    "resultCount": 50,
    "totalCount": 15432
  }
}
```

**Status Codes**:
- 200: Success
- 400: Bad request (invalid parameters)
- 500: Internal server error

**Performance Notes**:
- Search queries count against rate limits heavily
- Cache search results aggressively
- Use specific filters to reduce result set size
- Prefer slug+classId lookup when mod is known

#### GET /v1/mods/{modId}

Get a single mod by ID.

**Authentication**: Required

**Path Parameters**:
- `modId` (integer, required): Mod unique identifier

**Response**: 200 OK
```json
{
  "data": {
    "id": 238222,
    "gameId": 432,
    "name": "Just Enough Items (JEI)",
    "slug": "jei",
    "links": {
      "websiteUrl": "https://www.curseforge.com/minecraft/mc-mods/jei",
      "wikiUrl": null,
      "issuesUrl": "https://github.com/mezz/JustEnoughItems/issues",
      "sourceUrl": "https://github.com/mezz/JustEnoughItems"
    },
    "summary": "JEI is an item and recipe viewing mod for Minecraft",
    "status": 6,
    "downloadCount": 425789012,
    "isFeatured": true,
    "primaryCategoryId": 423,
    "categories": [...],
    "classId": 6,
    "authors": [...],
    "logo": {...},
    "screenshots": [],
    "mainFileId": 5284115,
    "latestFiles": [...],
    "latestFilesIndexes": [...],
    "latestEarlyAccessFilesIndexes": [],
    "dateCreated": "2016-05-15T10:00:00Z",
    "dateModified": "2024-01-10T15:30:00Z",
    "dateReleased": "2024-01-10T15:30:00Z",
    "allowModDistribution": true,
    "gamePopularityRank": 1,
    "isAvailable": true,
    "thumbsUpCount": 8542,
    "rating": 4.8
  }
}
```

**Status Codes**:
- 200: Success
- 404: Mod not found
- 500: Internal server error

#### POST /v1/mods

Get multiple mods by IDs. Bulk endpoint for fetching multiple mods efficiently.

**Authentication**: Required

**Request Body**:
```json
{
  "modIds": [238222, 419699, 32274],
  "filterPcOnly": true
}
```

**Body Parameters**:
- `modIds` (integer[], required): Array of mod IDs (all must belong to same game)
- `filterPcOnly` (boolean, optional): Filter for PC-compatible mods only

**Response**: 200 OK
```json
{
  "data": [
    {
      "id": 238222,
      "gameId": 432,
      "name": "Just Enough Items (JEI)",
      ...
    },
    {
      "id": 419699,
      "gameId": 432,
      "name": "Bookshelf",
      ...
    }
  ]
}
```

**Status Codes**:
- 200: Success
- 400: Bad request (modIds from different games, invalid format)
- 500: Internal server error

**Rate Limit Optimization**: Preferred over multiple GET /v1/mods/{modId} calls

#### POST /v1/mods/featured

Get featured, popular, and recently updated mods for a game.

**Authentication**: Required

**Request Body**:
```json
{
  "gameId": 432,
  "excludedModIds": [238222, 419699],
  "gameVersionTypeId": 1
}
```

**Body Parameters**:
- `gameId` (integer, required): Game to fetch featured mods for
- `excludedModIds` (integer[], optional): Mod IDs to exclude from results
- `gameVersionTypeId` (integer, optional): Filter by version type

**Response**: 200 OK
```json
{
  "data": {
    "featured": [
      {...mod object...},
      {...mod object...}
    ],
    "popular": [
      {...mod object...},
      {...mod object...}
    ],
    "recentlyUpdated": [
      {...mod object...},
      {...mod object...}
    ]
  }
}
```

**Status Codes**:
- 200: Success
- 400: Bad request (invalid gameId)
- 404: Game not found
- 500: Internal server error

**Use Case**: Homepage/discovery features, mod launcher landing pages

#### GET /v1/mods/{modId}/description

Get the full HTML description of a mod.

**Authentication**: Required

**Path Parameters**:
- `modId` (integer, required): Mod unique identifier

**Query Parameters**:
- `raw` (boolean, optional): Return raw markdown/HTML
- `stripped` (boolean, optional): Strip HTML tags
- `markup` (boolean, optional): Return with markup preserved

**Response**: 200 OK
```json
{
  "data": "<h2>Just Enough Items</h2><p>JEI is an item and recipe viewing mod...</p>"
}
```

**Status Codes**:
- 200: Success
- 404: Mod not found
- 500: Internal server error

**Content Format**: HTML with embedded images, links, formatting

### Files

#### GET /v1/mods/{modId}/files/{fileId}

Get a single file of a mod.

**Authentication**: Required

**Path Parameters**:
- `modId` (integer, required): Mod unique identifier
- `fileId` (integer, required): File unique identifier

**Response**: 200 OK
```json
{
  "data": {
    "id": 5284115,
    "gameId": 432,
    "modId": 238222,
    "isAvailable": true,
    "displayName": "jei-1.20.4-17.0.0.60.jar",
    "fileName": "jei-1.20.4-17.0.0.60.jar",
    "releaseType": 1,
    "fileStatus": 4,
    "hashes": [
      {
        "value": "a3f8b7c2d1e5f6a9b8c7d6e5f4a3b2c1",
        "algo": 2
      },
      {
        "value": "9f8e7d6c5b4a3928374650192837465",
        "algo": 1
      }
    ],
    "fileDate": "2024-01-10T15:30:00Z",
    "fileLength": 1248576,
    "downloadCount": 12456,
    "fileSizeOnDisk": 1248576,
    "downloadUrl": "https://edge.forgecdn.net/files/5284/115/jei-1.20.4-17.0.0.60.jar",
    "gameVersions": ["1.20.4", "Forge"],
    "sortableGameVersions": [
      {
        "gameVersionName": "1.20.4",
        "gameVersionPadded": "0000000001.0000000020.0000000004",
        "gameVersion": "1.20.4",
        "gameVersionReleaseDate": "2023-12-07T14:00:00Z",
        "gameVersionTypeId": 1
      }
    ],
    "dependencies": [
      {
        "modId": 419699,
        "relationType": 3
      }
    ],
    "exposeAsAlternative": false,
    "parentProjectFileId": null,
    "alternateFileId": null,
    "isServerPack": false,
    "serverPackFileId": null,
    "isEarlyAccessContent": false,
    "earlyAccessEndDate": null,
    "fileFingerprint": 2841256732,
    "modules": [
      {
        "name": "META-INF",
        "fingerprint": 3052645487
      }
    ]
  }
}
```

**Status Codes**:
- 200: Success
- 404: Mod or file not found
- 500: Internal server error

#### GET /v1/mods/{modId}/files

Get all files for a mod with optional filtering.

**Authentication**: Required

**Path Parameters**:
- `modId` (integer, required): Mod unique identifier

**Query Parameters**:
- `gameVersion` (string, optional): Filter by game version (e.g., "1.20.4")
- `modLoaderType` (ModLoaderType, optional): Filter by mod loader
  - Enum: 0 (Any), 1 (Forge), 2 (Cauldron), 3 (LiteLoader), 4 (Fabric), 5 (Quilt), 6 (NeoForge)
- `gameVersionTypeId` (integer, optional): Filter by version type
- `index` (integer, optional): Pagination index
- `pageSize` (integer, optional): Results per page (default/max: 50)

**Response**: 200 OK
```json
{
  "data": [
    {
      "id": 5284115,
      "gameId": 432,
      "modId": 238222,
      ...
    },
    {
      "id": 5273098,
      "gameId": 432,
      "modId": 238222,
      ...
    }
  ],
  "pagination": {
    "index": 0,
    "pageSize": 50,
    "resultCount": 50,
    "totalCount": 342
  }
}
```

**Status Codes**:
- 200: Success
- 404: Mod not found
- 500: Internal server error

**Use Case**: Version history browsing, finding files for specific game versions

#### POST /v1/mods/files

Get multiple files by IDs. Bulk endpoint for efficient batch retrieval.

**Authentication**: Required

**Request Body**:
```json
{
  "fileIds": [5284115, 5273098, 5264821]
}
```

**Body Parameters**:
- `fileIds` (integer[], required): Array of file IDs to retrieve

**Response**: 200 OK
```json
{
  "data": [
    {
      "id": 5284115,
      "gameId": 432,
      "modId": 238222,
      ...
    },
    {
      "id": 5273098,
      "gameId": 432,
      "modId": 238222,
      ...
    }
  ]
}
```

**Status Codes**:
- 200: Success
- 400: Bad request (invalid fileIds format)
- 404: One or more files not found
- 500: Internal server error

**Rate Limit Optimization**: Preferred over multiple GET calls

#### GET /v1/mods/{modId}/files/{fileId}/changelog

Get changelog for a specific file in HTML format.

**Authentication**: Required

**Path Parameters**:
- `modId` (integer, required): Mod unique identifier
- `fileId` (integer, required): File unique identifier

**Response**: 200 OK
```json
{
  "data": "<h3>Changes in 17.0.0.60</h3><ul><li>Fixed recipe display bug</li><li>Updated for 1.20.4</li></ul>"
}
```

**Status Codes**:
- 200: Success
- 404: Mod, file, or changelog not found
- 500: Internal server error

#### GET /v1/mods/{modId}/files/{fileId}/download-url

Get download URL for a specific file.

**Authentication**: Required

**Path Parameters**:
- `modId` (integer, required): Mod unique identifier
- `fileId` (integer, required): File unique identifier

**Response**: 200 OK
```json
{
  "data": "https://edge.forgecdn.net/files/5284/115/jei-1.20.4-17.0.0.60.jar"
}
```

**Status Codes**:
- 200: Success
- 404: Mod or file not found
- 500: Internal server error

**Note**: Download URLs also available directly in file object `downloadUrl` field

### Fingerprints

Fingerprint-based file identification using MurmurHash2 algorithm. Used for identifying installed mods and detecting file matches.

#### POST /v1/fingerprints/{gameId}

Get mod files matching fingerprints for a specific game.

**Authentication**: Required

**Path Parameters**:
- `gameId` (integer, required): Game ID for fingerprint matching

**Request Body**:
```json
{
  "fingerprints": [2841256732, 3052645487, 2156847392]
}
```

**Body Parameters**:
- `fingerprints` (integer[], required): Array of MurmurHash2 fingerprints

**Response**: 200 OK
```json
{
  "data": {
    "isCacheBuilt": true,
    "exactMatches": [
      {
        "id": 2841256732,
        "file": {
          "id": 5284115,
          "gameId": 432,
          "modId": 238222,
          ...
        },
        "latestFiles": [...]
      }
    ],
    "exactFingerprints": [2841256732],
    "partialMatches": [
      {
        "id": 3052645487,
        "file": {...},
        "latestFiles": [...]
      }
    ],
    "partialMatchFingerprints": {
      "5284115": [3052645487, 2156847392]
    },
    "installedFingerprints": [2841256732],
    "unmatchedFingerprints": []
  }
}
```

**Response Fields**:
- `isCacheBuilt`: Whether fingerprint cache is ready (may be false initially)
- `exactMatches`: Files where entire file fingerprint matches
- `exactFingerprints`: Fingerprints with exact matches
- `partialMatches`: Files where module fingerprints match
- `partialMatchFingerprints`: Map of fileId to matching module fingerprints
- `installedFingerprints`: All fingerprints found (exact or partial)
- `unmatchedFingerprints`: Fingerprints with no matches

**Status Codes**:
- 200: Success
- 400: Bad request (invalid fingerprints)
- 503: Service unavailable (cache not built, retry)

**Fingerprint Algorithm**: MurmurHash2 with seed 1, excluding whitespace characters (\x09, \x0a, \x0d, \x20)

#### POST /v1/fingerprints

Get mod files matching fingerprints (game-agnostic version).

**Authentication**: Required

**Request Body**:
```json
{
  "fingerprints": [2841256732, 3052645487]
}
```

**Response**: Same structure as POST /v1/fingerprints/{gameId}

**Status Codes**:
- 200: Success
- 400: Bad request
- 503: Service unavailable (cache building)

**Use Case**: When game ID unknown or searching across multiple games

#### POST /v1/fingerprints/fuzzy/{gameId}

Fuzzy fingerprint matching for mod folders/archives.

**Authentication**: Required

**Path Parameters**:
- `gameId` (integer, required): Game ID for matching

**Request Body**:
```json
{
  "gameId": 432,
  "fingerprints": [
    {
      "foldername": "jei-1.20.4",
      "fingerprints": [2841256732, 3052645487, 2156847392]
    },
    {
      "foldername": "bookshelf-common-1.20.4",
      "fingerprints": [1928374650, 5647382910]
    }
  ]
}
```

**Response**: 200 OK
```json
{
  "data": {
    "fuzzyMatches": [
      {
        "id": 2841256732,
        "file": {
          "id": 5284115,
          "gameId": 432,
          "modId": 238222,
          ...
        },
        "latestFiles": [...],
        "fingerprints": [2841256732, 3052645487]
      }
    ]
  }
}
```

**Status Codes**:
- 200: Success
- 400: Bad request
- 503: Service unavailable

**Use Case**: Identifying mods from extracted modpack folders

#### POST /v1/fingerprints/fuzzy

Fuzzy fingerprint matching (game-agnostic).

**Request/Response**: Same as POST /v1/fingerprints/fuzzy/{gameId}

### Minecraft-Specific Endpoints

#### GET /v1/minecraft/version

Get all Minecraft versions.

**Authentication**: Required

**Query Parameters**:
- `sortDescending` (boolean, optional): Sort versions descending (newest first)

**Response**: 200 OK
```json
{
  "data": [
    {
      "id": 9990,
      "gameVersionId": 9990,
      "versionString": "1.20.4",
      "jarDownloadUrl": "https://piston-data.mojang.com/.../client.jar",
      "jsonDownloadUrl": "https://piston-meta.mojang.com/.../1.20.4.json",
      "approved": true,
      "dateModified": "2023-12-07T14:00:00Z",
      "gameVersionTypeId": 1,
      "gameVersionStatus": 2,
      "gameVersionTypeStatus": 2
    }
  ]
}
```

**Status Codes**:
- 200: Success
- 404: Not found
- 500: Internal server error

#### GET /v1/minecraft/version/{gameVersionString}

Get specific Minecraft version by version string.

**Authentication**: Required

**Path Parameters**:
- `gameVersionString` (string, required): Version identifier (e.g., "1.20.4")

**Response**: 200 OK
```json
{
  "data": {
    "id": 9990,
    "gameVersionId": 9990,
    "versionString": "1.20.4",
    "jarDownloadUrl": "https://piston-data.mojang.com/.../client.jar",
    "jsonDownloadUrl": "https://piston-meta.mojang.com/.../1.20.4.json",
    "approved": true,
    "dateModified": "2023-12-07T14:00:00Z",
    "gameVersionTypeId": 1,
    "gameVersionStatus": 2,
    "gameVersionTypeStatus": 2
  }
}
```

**Status Codes**:
- 200: Success
- 404: Version not found
- 500: Internal server error

#### GET /v1/minecraft/modloader

Get all Minecraft mod loaders.

**Authentication**: Required

**Query Parameters**:
- `version` (string, optional): Filter by Minecraft version
- `includeAll` (boolean, optional): Include all versions (including deprecated)

**Response**: 200 OK
```json
{
  "data": [
    {
      "name": "forge-49.0.31",
      "gameVersion": "1.20.4",
      "latest": true,
      "recommended": true,
      "dateModified": "2024-01-08T10:00:00Z",
      "type": 1
    },
    {
      "name": "fabric-0.15.3",
      "gameVersion": "1.20.4",
      "latest": true,
      "recommended": true,
      "dateModified": "2024-01-05T14:30:00Z",
      "type": 4
    }
  ]
}
```

**Mod Loader Types**:
- 0: Any
- 1: Forge
- 2: Cauldron
- 3: LiteLoader
- 4: Fabric
- 5: Quilt
- 6: NeoForge

**Status Codes**:
- 200: Success
- 404: Not found
- 500: Internal server error

#### GET /v1/minecraft/modloader/{modLoaderName}

Get specific mod loader details.

**Authentication**: Required

**Path Parameters**:
- `modLoaderName` (string, required): Mod loader name (e.g., "forge-49.0.31")

**Response**: 200 OK
```json
{
  "data": {
    "id": 4968,
    "gameVersionId": 9990,
    "minecraftGameVersionId": 9990,
    "forgeVersion": "49.0.31",
    "name": "forge-49.0.31",
    "type": 1,
    "downloadUrl": "https://maven.minecraftforge.net/.../forge-1.20.4-49.0.31-installer.jar",
    "filename": "forge-1.20.4-49.0.31-installer.jar",
    "installMethod": 1,
    "latest": true,
    "recommended": true,
    "approved": true,
    "dateModified": "2024-01-08T10:00:00Z",
    "mavenVersionString": "net.minecraftforge:forge:1.20.4-49.0.31",
    "versionJson": "{...minecraft version json...}",
    "librariesInstallLocation": "libraries",
    "minecraftVersion": "1.20.4",
    "additionalFilesJson": null,
    "modLoaderGameVersionId": 9990,
    "modLoaderGameVersionTypeId": 1,
    "modLoaderGameVersionStatus": 2,
    "modLoaderGameVersionTypeStatus": 2,
    "mcGameVersionId": 9990,
    "mcGameVersionTypeId": 1,
    "mcGameVersionStatus": 2,
    "mcGameVersionTypeStatus": 2,
    "installProfileJson": "{...forge install profile...}"
  }
}
```

**Status Codes**:
- 200: Success
- 404: Mod loader not found
- 500: Internal server error

## Response Schemas

### Core Enumerations

#### CoreStatus

Represents the publication status of content.

**Values**:
- `1`: Draft - In initial creation
- `2`: Test - In testing phase
- `3`: PendingReview - Awaiting moderation
- `4`: Rejected - Failed moderation
- `5`: Approved - Passed moderation but not published
- `6`: Live - Published and publicly available

#### CoreApiStatus

Represents API visibility status.

**Values**:
- `1`: Private - Only accessible via authorized API keys
- `2`: Public - Accessible to all API keys

#### ModLoaderType

Represents mod loader platforms.

**Values**:
- `0`: Any - No specific mod loader requirement
- `1`: Forge - Minecraft Forge
- `2`: Cauldron - Cauldron (legacy)
- `3`: LiteLoader - LiteLoader (legacy)
- `4`: Fabric - Fabric Loader
- `5`: Quilt - Quilt Loader
- `6`: NeoForge - NeoForged (Forge fork)

#### FileReleaseType

Represents file release stability level.

**Values**:
- `1`: Release - Stable release version
- `2`: Beta - Beta testing version
- `3`: Alpha - Alpha testing version (early access)

**Distribution Behavior**:
- Release: Distributed to all users
- Beta: Requires user opt-in to beta channel
- Alpha: Requires explicit alpha opt-in, highly restricted

#### FileRelationType

Represents dependency relationship types.

**Values**:
- `1`: EmbeddedLibrary - Library bundled within the file
- `2`: OptionalDependency - Optional mod enhancing functionality
- `3`: RequiredDependency - Mandatory mod for operation
- `4`: Tool - Development/build tool
- `5`: Incompatible - Conflicting mod that breaks functionality
- `6`: Include - Included in modpack/compilation

**String Equivalents** (Upload API):
- `embeddedLibrary`
- `optionalDependency`
- `requiredDependency`
- `tool`
- `incompatible`
- `include`

#### FileStatus

File processing and availability status.

**Values**:
- `1`: Processing - File uploaded, awaiting processing
- `2`: ChangesRequired - Rejected, needs author changes
- `3`: UnderReview - In moderation queue
- `4`: Approved - Approved and available
- `5`: Rejected - Permanently rejected
- `6`: MalwareDetected - Flagged as malware
- `7`: Deleted - Removed by author or admin
- `8`: Archived - Archived, no longer actively maintained
- `9`: Testing - In testing phase
- `10`: Released - Released (synonym for Approved)
- `11`: ReadyForReview - Ready for moderation
- `12`: Deprecated - Superseded by newer version
- `13`: Baking - Processing/indexing
- `14`: AwaitingPublishing - Approved, scheduled for publish
- `15`: FailedPublishing - Publish operation failed

#### HashAlgo

Hash algorithm used for file verification.

**Values**:
- `1`: SHA1 - SHA-1 hash (160-bit)
- `2`: MD5 - MD5 hash (128-bit)

**Usage**: Files include hashes array with both MD5 and SHA1 for verification

#### ModsSearchSortField

Sort fields for mod search.

**Values**:
- `1`: Featured - Featured status
- `2`: Popularity - Current popularity metrics
- `3`: LastUpdated - Most recently updated
- `4`: Name - Alphabetical by name
- `5`: Author - Alphabetical by author
- `6`: TotalDownloads - Lifetime download count
- `7`: Category - Category grouping
- `8`: GameVersion - Game version
- `9`: EarlyAccess - Early access status (descending only)
- `10`: FeaturedReleased - Featured with weighted scoring (descending only)
- `11`: RelativePopularity - Popularity relative to game (descending only)
- `12`: Fingerprint - Exact fingerprint match (descending only)

#### SortOrder

Sort direction.

**Values**:
- `asc`: Ascending order
- `desc`: Descending order

### Data Structures

#### Game

```json
{
  "id": 432,
  "name": "Minecraft",
  "slug": "minecraft",
  "dateModified": "2024-01-15T10:30:00Z",
  "assets": {
    "iconUrl": "https://media.forgecdn.net/.../icon.png",
    "tileUrl": "https://media.forgecdn.net/.../tile.png",
    "coverUrl": "https://media.forgecdn.net/.../cover.png"
  },
  "status": 6,
  "apiStatus": 2
}
```

**Fields**:
- `id` (integer): Unique game identifier
- `name` (string): Game display name
- `slug` (string): URL-safe game identifier
- `dateModified` (datetime): Last modification timestamp
- `assets` (object): Game visual assets
  - `iconUrl` (string): Small icon URL
  - `tileUrl` (string): Tile/thumbnail URL
  - `coverUrl` (string): Cover image URL
- `status` (CoreStatus): Publication status
- `apiStatus` (CoreApiStatus): API visibility status

#### Category

```json
{
  "id": 423,
  "gameId": 432,
  "name": "Map and Information",
  "slug": "map-information",
  "url": "https://www.curseforge.com/minecraft/mc-mods/map-information",
  "iconUrl": "https://media.forgecdn.net/.../icon.png",
  "dateModified": "2023-12-01T08:00:00Z",
  "isClass": false,
  "classId": 6,
  "parentCategoryId": 6,
  "displayIndex": 10
}
```

**Fields**:
- `id` (integer): Unique category identifier
- `gameId` (integer): Associated game ID
- `name` (string): Category display name
- `slug` (string): URL-safe category identifier
- `url` (string): Category page URL
- `iconUrl` (string): Category icon URL
- `dateModified` (datetime): Last modification timestamp
- `isClass` (boolean): True if top-level class, false if subcategory
- `classId` (integer, nullable): Parent class ID (null for classes)
- `parentCategoryId` (integer, nullable): Parent category ID
- `displayIndex` (integer): Sort order within parent

#### Mod

```json
{
  "id": 238222,
  "gameId": 432,
  "name": "Just Enough Items (JEI)",
  "slug": "jei",
  "links": {
    "websiteUrl": "https://www.curseforge.com/minecraft/mc-mods/jei",
    "wikiUrl": null,
    "issuesUrl": "https://github.com/mezz/JustEnoughItems/issues",
    "sourceUrl": "https://github.com/mezz/JustEnoughItems"
  },
  "summary": "JEI is an item and recipe viewing mod for Minecraft",
  "status": 6,
  "downloadCount": 425789012,
  "isFeatured": true,
  "primaryCategoryId": 423,
  "categories": [...],
  "classId": 6,
  "authors": [...],
  "logo": {...},
  "screenshots": [...],
  "mainFileId": 5284115,
  "latestFiles": [...],
  "latestFilesIndexes": [...],
  "latestEarlyAccessFilesIndexes": [...],
  "dateCreated": "2016-05-15T10:00:00Z",
  "dateModified": "2024-01-10T15:30:00Z",
  "dateReleased": "2024-01-10T15:30:00Z",
  "allowModDistribution": true,
  "gamePopularityRank": 1,
  "isAvailable": true,
  "thumbsUpCount": 8542,
  "rating": 4.8
}
```

**Fields**:
- `id` (integer): Unique mod identifier
- `gameId` (integer): Associated game ID
- `name` (string): Mod display name
- `slug` (string): URL-safe mod identifier
- `links` (object): External links
  - `websiteUrl` (string): CurseForge project page
  - `wikiUrl` (string, nullable): Wiki URL
  - `issuesUrl` (string, nullable): Issue tracker URL
  - `sourceUrl` (string, nullable): Source code URL
- `summary` (string): Short description
- `status` (CoreStatus): Publication status
- `downloadCount` (integer): Lifetime downloads
- `isFeatured` (boolean): Featured status
- `primaryCategoryId` (integer): Main category
- `categories` (Category[]): All assigned categories
- `classId` (integer): Project class (6=Mods, 8=Resource Packs, etc.)
- `authors` (ModAuthor[]): Mod authors
- `logo` (ModAsset): Project logo
- `screenshots` (ModAsset[]): Screenshot gallery
- `mainFileId` (integer): Primary/recommended file ID
- `latestFiles` (File[]): Recent file uploads (typically last 3)
- `latestFilesIndexes` (FileIndex[]): File index for quick lookups
- `latestEarlyAccessFilesIndexes` (FileIndex[]): Early access file index
- `dateCreated` (datetime): Project creation date
- `dateModified` (datetime): Last update timestamp
- `dateReleased` (datetime): Most recent file release
- `allowModDistribution` (boolean): Third-party distribution permitted
- `gamePopularityRank` (integer): Ranking within game (1=most popular)
- `isAvailable` (boolean): Currently available for download
- `thumbsUpCount` (integer): Positive rating count
- `rating` (number): Average rating (0.0-5.0)

#### File

```json
{
  "id": 5284115,
  "gameId": 432,
  "modId": 238222,
  "isAvailable": true,
  "displayName": "jei-1.20.4-17.0.0.60.jar",
  "fileName": "jei-1.20.4-17.0.0.60.jar",
  "releaseType": 1,
  "fileStatus": 4,
  "hashes": [
    {
      "value": "a3f8b7c2d1e5f6a9b8c7d6e5f4a3b2c1",
      "algo": 2
    },
    {
      "value": "9f8e7d6c5b4a3928374650192837465",
      "algo": 1
    }
  ],
  "fileDate": "2024-01-10T15:30:00Z",
  "fileLength": 1248576,
  "downloadCount": 12456,
  "fileSizeOnDisk": 1248576,
  "downloadUrl": "https://edge.forgecdn.net/files/5284/115/jei-1.20.4-17.0.0.60.jar",
  "gameVersions": ["1.20.4", "Forge"],
  "sortableGameVersions": [...],
  "dependencies": [...],
  "exposeAsAlternative": false,
  "parentProjectFileId": null,
  "alternateFileId": null,
  "isServerPack": false,
  "serverPackFileId": null,
  "isEarlyAccessContent": false,
  "earlyAccessEndDate": null,
  "fileFingerprint": 2841256732,
  "modules": [...]
}
```

**Fields**:
- `id` (integer): Unique file identifier
- `gameId` (integer): Associated game ID
- `modId` (integer): Parent mod ID
- `isAvailable` (boolean): Currently downloadable
- `displayName` (string): User-facing filename
- `fileName` (string): Actual filename
- `releaseType` (FileReleaseType): Stability level
- `fileStatus` (FileStatus): Processing/approval status
- `hashes` (FileHash[]): Verification hashes
- `fileDate` (datetime): Upload timestamp
- `fileLength` (integer): File size in bytes (compressed)
- `downloadCount` (integer): Download count
- `fileSizeOnDisk` (integer): Extracted size in bytes
- `downloadUrl` (string): CDN download URL
- `gameVersions` (string[]): Compatible game versions
- `sortableGameVersions` (SortableGameVersion[]): Structured version data
- `dependencies` (FileDependency[]): Mod dependencies
- `exposeAsAlternative` (boolean): Show as alternative download
- `parentProjectFileId` (integer, nullable): Parent file for alternatives
- `alternateFileId` (integer, nullable): Alternative file ID
- `isServerPack` (boolean): Server-side modpack
- `serverPackFileId` (integer, nullable): Associated server pack
- `isEarlyAccessContent` (boolean): Early access release
- `earlyAccessEndDate` (datetime, nullable): Early access expiration
- `fileFingerprint` (integer): MurmurHash2 file fingerprint
- `modules` (FileModule[]): Internal module fingerprints

#### FileDependency

```json
{
  "modId": 419699,
  "relationType": 3
}
```

**Fields**:
- `modId` (integer): Dependent mod ID
- `relationType` (FileRelationType): Dependency type

#### FileHash

```json
{
  "value": "a3f8b7c2d1e5f6a9b8c7d6e5f4a3b2c1",
  "algo": 2
}
```

**Fields**:
- `value` (string): Hex-encoded hash value
- `algo` (HashAlgo): Hash algorithm

#### FileModule

```json
{
  "name": "META-INF",
  "fingerprint": 3052645487
}
```

**Fields**:
- `name` (string): Module/folder name within JAR/ZIP
- `fingerprint` (integer): MurmurHash2 fingerprint of module contents

#### SortableGameVersion

```json
{
  "gameVersionName": "1.20.4",
  "gameVersionPadded": "0000000001.0000000020.0000000004",
  "gameVersion": "1.20.4",
  "gameVersionReleaseDate": "2023-12-07T14:00:00Z",
  "gameVersionTypeId": 1
}
```

**Fields**:
- `gameVersionName` (string): Display name
- `gameVersionPadded` (string): Zero-padded for sorting
- `gameVersion` (string): Version string
- `gameVersionReleaseDate` (datetime): Version release date
- `gameVersionTypeId` (integer): Version type (1=Minecraft Java, etc.)

#### FileIndex

Quick lookup structure for finding files by version/modloader.

```json
{
  "gameVersion": "1.20.4",
  "fileId": 5284115,
  "filename": "jei-1.20.4-17.0.0.60.jar",
  "releaseType": 1,
  "gameVersionTypeId": 1,
  "modLoader": 1
}
```

**Fields**:
- `gameVersion` (string): Game version
- `fileId` (integer): File ID
- `filename` (string): Filename
- `releaseType` (FileReleaseType): Release type
- `gameVersionTypeId` (integer): Version type
- `modLoader` (ModLoaderType): Mod loader type

#### ModAuthor

```json
{
  "id": 166630,
  "name": "mezz",
  "url": "https://www.curseforge.com/members/mezz"
}
```

**Fields**:
- `id` (integer): Author user ID
- `name` (string): Display name
- `url` (string): Author profile URL

#### ModAsset

```json
{
  "id": 382635,
  "modId": 238222,
  "title": "JEI Logo",
  "description": "Main logo for Just Enough Items",
  "thumbnailUrl": "https://media.forgecdn.net/.../thumbnail.png",
  "url": "https://media.forgecdn.net/.../full.png"
}
```

**Fields**:
- `id` (integer): Asset ID
- `modId` (integer): Parent mod ID
- `title` (string): Asset title
- `description` (string): Asset description
- `thumbnailUrl` (string): Thumbnail URL (typically 256x256)
- `url` (string): Full-size image URL

#### Pagination

```json
{
  "index": 0,
  "pageSize": 50,
  "resultCount": 50,
  "totalCount": 1523
}
```

**Fields**:
- `index` (integer): Current page starting index
- `pageSize` (integer): Requested page size
- `resultCount` (integer): Actual items returned on this page
- `totalCount` (integer): Total matching items (may exceed 10,000 limit)

## Dependencies & Compatibility Resolution

### Dependency Types

CurseForge supports six dependency relationship types:

1. **RequiredDependency** (relationType: 3)
   - Mandatory for mod to function
   - Must be installed alongside mod
   - Transitive: dependency chains must be fully resolved

2. **OptionalDependency** (relationType: 2)
   - Enhances functionality but not required
   - User/launcher discretion to install
   - Common for integration mods

3. **EmbeddedLibrary** (relationType: 1)
   - Library bundled within the mod file
   - No separate installation needed
   - Licensing implications - check mod page

4. **Tool** (relationType: 4)
   - Development/build-time dependency
   - Not required at runtime
   - Examples: build scripts, code generators

5. **Incompatible** (relationType: 5)
   - Conflicting mod that breaks functionality
   - Must NOT be installed together
   - Check before installation

6. **Include** (relationType: 6)
   - Included in modpack/compilation
   - Already bundled in download
   - No separate action needed

### Dependency Resolution Process

**Step 1: Extract Dependencies**

From file object:
```json
{
  "dependencies": [
    {
      "modId": 419699,
      "relationType": 3
    }
  ]
}
```

**Step 2: Fetch Dependent Mods**

Use POST /v1/mods with modIds array:
```json
{
  "modIds": [419699]
}
```

**Step 3: Select Compatible Files**

For each dependency:
1. Filter files by gameVersion match
2. Filter by modLoaderType match
3. Filter by releaseType (respect user's alpha/beta preferences)
4. Select most recent file matching criteria

**Step 4: Recursive Resolution**

1. Process dependencies of dependencies
2. Build dependency graph
3. Detect circular dependencies (shouldn't occur, but validate)
4. Flatten to installation list

**Step 5: Conflict Detection**

1. Check for incompatible mods (relationType: 5)
2. Verify no version conflicts
3. Alert user to incompatibilities

### Mod Loader Compatibility

Files specify compatible mod loaders in two ways:

**1. gameVersions Array**
```json
{
  "gameVersions": ["1.20.4", "Forge", "Java 17"]
}
```

String matching: "Forge", "Fabric", "Quilt", "NeoForge"

**2. latestFilesIndexes**
```json
{
  "latestFilesIndexes": [
    {
      "gameVersion": "1.20.4",
      "fileId": 5284115,
      "modLoader": 1
    }
  ]
}
```

ModLoaderType enum (1=Forge, 4=Fabric, 5=Quilt, 6=NeoForge)

### Version Compatibility

**Game Version Matching**:

Files use `sortableGameVersions` for structured version data:
```json
{
  "sortableGameVersions": [
    {
      "gameVersionName": "1.20.4",
      "gameVersionPadded": "0000000001.0000000020.0000000004",
      "gameVersion": "1.20.4",
      "gameVersionReleaseDate": "2023-12-07T14:00:00Z",
      "gameVersionTypeId": 1
    }
  ]
}
```

**Version Comparison**:
1. Use `gameVersionPadded` for numerical sorting
2. Match exact version or version range
3. Consider version type (Java vs Bedrock)

**Multi-Version Files**:

Files may support multiple game versions:
```json
{
  "gameVersions": ["1.20.3", "1.20.4"]
}
```

Select file if any version matches target.

### Modpack Dependency Resolution

Modpacks have complex dependency trees:

**1. Parse Modpack Manifest**

Modpacks include file listing:
```json
{
  "files": [
    {
      "projectID": 238222,
      "fileID": 5284115,
      "required": true
    }
  ]
}
```

**2. Batch Fetch Files**

Use POST /v1/mods/files:
```json
{
  "fileIds": [5284115, 5273098, 5264821]
}
```

**3. Resolve File Dependencies**

Each file may have dependencies - resolve recursively.

**4. Deduplicate**

Remove duplicate mods (keep newest version or user preference).

**5. Validate**

- All required dependencies present
- No incompatible combinations
- Mod loader consistency

### Best Practices

**Caching**:
- Cache mod metadata and dependency graphs
- Update cache on game version change
- Invalidate after 24 hours or manual refresh

**Parallel Resolution**:
- Fetch multiple mods in parallel using POST /v1/mods
- Batch file lookups with POST /v1/mods/files
- Minimize sequential API calls

**User Choice**:
- For optional dependencies, prompt user
- Respect user preferences for alpha/beta files
- Allow manual override of automatic selection

**Error Handling**:
- Handle missing dependencies gracefully
- Provide clear error messages
- Suggest alternatives when available

## Downloads & Caching

### CDN Infrastructure

**Primary CDN**: edge.forgecdn.net

**URL Structure**:
```
https://edge.forgecdn.net/files/{fileIdSegment1}/{fileIdSegment2}/{fileName}
```

**URL Construction**:

Given fileId `5284115` and fileName `jei-1.20.4-17.0.0.60.jar`:
1. Convert fileId to string: "5284115"
2. Split into segments: first 4 digits, remaining digits
3. Segment 1: "5284" (first 4 digits)
4. Segment 2: "115" (remaining digits)
5. URL: `https://edge.forgecdn.net/files/5284/115/jei-1.20.4-17.0.0.60.jar`

**Alternative**: Use GET /v1/mods/{modId}/files/{fileId}/download-url for canonical URL

### Download Authentication

**No Authentication Required**: File downloads from CDN do not require API key

**Redirect Handling**: Some URLs may redirect, follow HTTP 301/302

**User-Agent**: Set identifiable User-Agent for analytics and debugging

### File Verification

**Hash Verification**:

Files include MD5 and SHA1 hashes:
```json
{
  "hashes": [
    {
      "value": "a3f8b7c2d1e5f6a9b8c7d6e5f4a3b2c1",
      "algo": 2
    },
    {
      "value": "9f8e7d6c5b4a3928374650192837465",
      "algo": 1
    }
  ]
}
```

**Verification Process**:
1. Download file
2. Compute MD5 hash (algo: 2)
3. Compare with hash value from API
4. If mismatch, retry download or report corruption
5. Optionally verify SHA1 (algo: 1) for additional confidence

**Fingerprint Verification**:

MurmurHash2 fingerprint for identifying files:
```json
{
  "fileFingerprint": 2841256732
}
```

**Fingerprint Algorithm**:
- MurmurHash2 with seed 1
- Exclude whitespace: \x09 (tab), \x0a (LF), \x0d (CR), \x20 (space)
- Apply to normalized file contents

**Use Case**: Detect installed mods via POST /v1/fingerprints

### Caching Strategy

#### Client-Side Caching

**File Caching**:
1. Cache downloaded files indefinitely (immutable content)
2. Index by fileId + fileName
3. Verify hash before use
4. Invalidate only on hash mismatch

**Metadata Caching**:
1. Mod metadata: 1-24 hours
2. File listings: 1 hour
3. Search results: 5-15 minutes
4. Game/category lists: 24 hours

**Cache Keys**:
- Mod: `mod:{modId}`
- File: `file:{modId}:{fileId}`
- Search: `search:{gameId}:{hash(params)}`
- Files list: `files:{modId}:{gameVersion}:{modLoader}`

#### HTTP Caching Headers

**ETag Support**: CDN responses include ETag header

**ETag Usage**:
```http
GET /files/5284/115/jei-1.20.4-17.0.0.60.jar HTTP/1.1
Host: edge.forgecdn.net
If-None-Match: "a3f8b7c2d1e5f6a9b8c7d6e5f4a3b2c1"
```

**Response**:
- 304 Not Modified: File unchanged, use cached version
- 200 OK: File changed, download new version

**ETag Format**: Typically MD5 hash of file (but may vary)

**Note**: ETag correspondence to MD5 not guaranteed for large files

**Conditional Requests**:
```http
GET /files/5284/115/jei-1.20.4-17.0.0.60.jar HTTP/1.1
Host: edge.forgecdn.net
If-Modified-Since: Wed, 10 Jan 2024 15:30:00 GMT
```

**Cache-Control**: Not consistently provided, implement application-level caching

### Bandwidth Optimization

**Parallel Downloads**:
- Limit concurrent downloads (4-8 connections)
- Use HTTP/2 for connection multiplexing
- Implement download queuing

**Resume Support**:
- Use Range requests for large files
- Store partial downloads with checksums
- Resume from last byte on connection failure

**Compression**:
- Files are pre-compressed (JAR/ZIP format)
- Do not request gzip/deflate encoding
- Accept files as-is

**Deduplication**:
- Check local cache before downloading
- Use fingerprints to detect identical files
- Share files across modpacks when possible

### Download Limits

**Rate Limiting**: CDN downloads not subject to API rate limits

**Bandwidth Limits**: No publicly documented bandwidth caps

**Concurrent Connections**: Limit to 8 connections per client

**Retry Strategy**:
1. Initial request
2. Wait 5 seconds, retry
3. Wait 15 seconds, retry
4. Wait 60 seconds, retry
5. Fail with clear error message

**Timeout**: 60 seconds for connection, 300 seconds total per file

### Alternative CDN Domains

**Historical Domains**:
- media.forgecdn.net (assets/images)
- mediafilez.forgecdn.net (legacy files)

**Redirect Behavior**: Old URLs may redirect to edge.forgecdn.net

**Compatibility**: Support 301/302 redirects in download client

### Download Progress & UX

**File Size**:
- Use `fileLength` for progress calculation
- Display in MB/GB for user
- Show download speed and ETA

**Progress Updates**:
- Update UI every 100KB or 250ms
- Use `fileSizeOnDisk` for post-download space estimation
- Display total progress for batch downloads

**Error Recovery**:
- Detect corrupted downloads via hash mismatch
- Automatic retry with exponential backoff
- Clear user messaging on failure

## Error Handling

### HTTP Status Codes

#### 2xx Success

**200 OK**
- Request successful
- Response contains requested data
- No further action needed

**304 Not Modified** (Conditional Requests)
- Resource unchanged since last fetch
- Use cached version
- Common with ETag/If-Modified-Since

#### 4xx Client Errors

**400 Bad Request**
- Invalid parameters
- Malformed request body
- Parameter constraint violations (e.g., index + pageSize > 10,000)

**Example Response**:
```json
{
  "error": "Invalid parameter: index + pageSize must not exceed 10,000"
}
```

**Resolution**:
- Validate parameters before request
- Check API documentation for constraints
- Fix request and retry

**403 Forbidden**
- Invalid or missing API key
- Rate limit exceeded
- Access denied to private resource

**Error Messages**:
- "Access forbidden or rate-limit exceeded"
- "Invalid API key"

**Resolution**:
- Verify API key is valid and included in x-api-key header
- If rate limited, implement backoff and retry
- Check resource permissions

**404 Not Found**
- Resource does not exist
- Invalid ID (mod, file, game)
- Resource deleted or never existed

**Resolution**:
- Verify ID is correct
- Check resource status (may be deleted)
- Handle gracefully with user-friendly message

#### 5xx Server Errors

**500 Internal Server Error**
- Server-side error
- Unexpected exception
- Database or service failure

**Resolution**:
- Retry with exponential backoff
- Report to CurseForge if persistent
- Fall back to cached data if available

**503 Service Unavailable**
- Fingerprint cache not built (common on fingerprint endpoints)
- Server maintenance
- Temporary service disruption

**Special Case - Fingerprints**:
```json
{
  "data": {
    "isCacheBuilt": false
  }
}
```

**Resolution**:
- Wait 10-30 seconds
- Retry request
- May take several retries during cache rebuild
- Max retries: 5-10 before failing

### Error Response Formats

**Standard Error**:
```json
{
  "error": "Rate limit exceeded",
  "message": "API rate limit has been exceeded. Please try again later."
}
```

**Validation Error**:
```json
{
  "error": "Validation failed",
  "errors": [
    {
      "field": "gameId",
      "message": "gameId is required"
    }
  ]
}
```

**No Response Body**: Some errors (especially 403, 404) may return empty body

### Retry Strategies

#### Exponential Backoff

**Implementation**:
```
attempt 1: immediate
attempt 2: wait 1s
attempt 3: wait 2s
attempt 4: wait 4s
attempt 5: wait 8s
attempt 6: wait 16s
max wait: 60s
```

**Jitter**: Add random 0-1000ms to prevent thundering herd

**Max Retries**: 5 attempts for transient errors

#### Rate Limit Backoff

**On 403 with rate limit message**:
```
attempt 1: wait 60s
attempt 2: wait 120s
attempt 3: wait 300s
attempt 4: fail
```

**Strategy**:
- Longer initial backoff (60s)
- Fewer retry attempts (3-4)
- Consider caching to reduce API calls

#### Fingerprint Cache Backoff

**On 503 with isCacheBuilt: false**:
```
attempt 1: wait 10s
attempt 2: wait 15s
attempt 3: wait 20s
attempt 4: wait 30s
attempt 5: wait 30s
max attempts: 10
```

**Special handling**: More retries acceptable as cache builds

### Error Logging & Debugging

**Log Details**:
- Request URL and method
- Request headers (excluding API key)
- Request body (sanitized)
- Response status code
- Response body
- Timestamp
- Retry attempt number

**Correlation IDs**: CurseForge does not provide request IDs in responses

**Debug Headers**: No special debug headers available

### Common Error Scenarios

#### "mod IDs from different games"

**Error**: POST /v1/mods with modIds from multiple games

**Resolution**: Group modIds by game, make separate requests

#### "index + pageSize > 10,000"

**Error**: Pagination constraint violated

**Resolution**: Adjust parameters to satisfy constraint, or accept limit

#### "Fingerprint cache building"

**Error**: 503 on fingerprint endpoints

**Resolution**: Retry with backoff, cache builds asynchronously

#### "File not available"

**Error**: File with isAvailable: false

**Resolution**: Check fileStatus, may be deleted/rejected/malware

#### "API key forbidden"

**Error**: 403 on all requests

**Resolution**: Verify API key, check console.curseforge.com for key status

### Best Practices

**Graceful Degradation**:
- Use cached data when API unavailable
- Display last known state to user
- Show clear offline/error indicators

**User Communication**:
- Avoid technical jargon in error messages
- Provide actionable guidance
- Include support contact for persistent issues

**Monitoring**:
- Track error rates by endpoint
- Alert on sustained 5xx errors
- Monitor rate limit frequency

**Circuit Breaker**:
- After N consecutive failures, stop requests temporarily
- Exponentially increase break duration
- Resume with health check request

## Best Practices

### Rate Limit Compliance

**Minimize Requests**:
- Cache aggressively (see Caching section)
- Batch requests using POST endpoints
- Prefer specific queries over broad searches

**Efficient Querying**:
- Use POST /v1/mods instead of multiple GET /v1/mods/{modId}
- Use POST /v1/mods/files instead of multiple file requests
- Fetch only necessary fields (no field selection, cache entire responses)

**Request Spacing**:
- Space requests 100-500ms apart when possible
- Avoid burst patterns (sudden 50 requests)
- Implement request queue with rate limiting

### Caching Guidelines

**What to Cache**:
- Mod metadata: 1-24 hours
- File listings: 30-60 minutes
- Search results: 5-15 minutes
- Categories/games: 24 hours
- Downloaded files: Indefinitely (immutable)

**Cache Invalidation**:
- Time-based expiration
- Manual refresh on user request
- Invalidate on hash mismatch

**Cache Storage**:
- Use persistent storage (disk, database)
- Index by composite keys (modId+version+loader)
- Implement LRU eviction for size limits

### Search Optimization

**Reduce Search Queries**:
- Use autocomplete debouncing (300ms delay)
- Require minimum 3 characters
- Cache popular searches
- Prefer slug lookup when mod known

**Efficient Filters**:
- Apply specific filters (gameVersion, modLoaderType)
- Limit categoryIds to relevant categories
- Use gameVersionTypeId when targeting specific version type

**Result Pagination**:
- Default pageSize=20 for UI (not 50)
- Implement infinite scroll or "load more" pattern
- Avoid deep pagination (beyond page 100)

### Dependency Resolution

**Batch Fetching**:
- Collect all dependency modIds first
- Single POST /v1/mods call with all IDs
- Parallel file lookups with POST /v1/mods/files

**Graph Optimization**:
- Build dependency graph in memory
- Detect cycles early
- Flatten to minimal installation list
- Cache resolved dependency graphs

**User Experience**:
- Show dependency tree to user
- Allow opt-out of optional dependencies
- Pre-select common dependencies
- Warn about incompatibilities

### Download Management

**Connection Limits**:
- Max 8 concurrent downloads
- Queue additional downloads
- Prioritize by user action vs. background

**Verification**:
- Always verify MD5 hash
- Optionally verify SHA1 for critical files
- Retry on hash mismatch (max 3 times)
- Report persistent mismatches

**Storage**:
- Organize by game/version/modloader
- Use fileId in storage key for uniqueness
- Implement disk space monitoring
- Clean old versions periodically

### API Key Management

**Security**:
- Never commit API keys to version control
- Use environment variables or secure vaults
- Rotate keys periodically (quarterly)
- Monitor usage in console.curseforge.com

**Multi-Environment**:
- Separate keys for dev/staging/production
- Test with dev key before production
- Track usage per key

**Key Distribution**:
- For open-source projects, document key requirement
- Do not include key in releases
- Provide clear key setup instructions

### User Agent

**Format**:
```
User-Agent: YourAppName/1.0.0 (contact@yourdomain.com)
```

**Components**:
- Application name and version
- Contact email for issues
- Optional: Platform/OS info

**Example**:
```
User-Agent: ModManager/2.3.1 (https://github.com/yourrepo/modmanager) Java/17
```

**Benefits**:
- CurseForge can contact on issues
- Analytics and debugging
- Professional presentation

### Error Handling

**Graceful Degradation**:
- Fall back to cached data when API fails
- Display stale data with timestamp
- Implement offline mode for critical functions

**User Messaging**:
- Translate technical errors to user-friendly messages
- Provide context and next steps
- Avoid exposing API internals

**Logging**:
- Log all errors with context
- Include request/response details (sanitize API key)
- Track error frequency for monitoring

### Performance Optimization

**Parallel Requests**:
- Fetch independent resources concurrently
- Use Promise.all or equivalent
- Respect connection limits (8 max)

**Lazy Loading**:
- Load detailed data only when needed
- Fetch file lists on demand
- Defer optional data (screenshots, changelogs)

**Pagination**:
- Implement virtual scrolling for large lists
- Prefetch next page on scroll
- Limit initial page size to 20-30 items

### Testing

**Mock API**:
- Implement mock API for development
- Use recorded responses for consistency
- Test rate limit handling

**Integration Tests**:
- Test with real API in CI (separate API key)
- Verify pagination, search, downloads
- Test error scenarios

**Rate Limit Testing**:
- Intentionally trigger rate limits in testing
- Verify backoff and retry logic
- Ensure graceful degradation

### Compliance

**Terms of Service**:
- Review CurseForge 3rd Party API Terms
- Respect allowModDistribution flag
- Credit CurseForge and mod authors
- Do not circumvent API for scraping

**Attribution**:
- Display mod author names
- Link back to CurseForge project pages
- Show "Powered by CurseForge" or similar

**Content Policy**:
- Respect mod author licensing
- Do not redistribute mods violating ToS
- Report malware/inappropriate content

## Common Patterns

### Pattern: Get Mod by Slug

**Scenario**: User searches for "JEI", you want mod details

**Implementation**:
```
1. GET /v1/mods/search?gameId=432&slug=jei&classId=6
2. Extract mod from data[0]
3. Cache result for 1 hour
```

**Optimization**: Combined slug+classId lookup is unique

### Pattern: Find Compatible Files

**Scenario**: User on 1.20.4 Forge, needs compatible JEI version

**Implementation**:
```
1. GET /v1/mods/238222/files?gameVersion=1.20.4&modLoaderType=1
2. Filter by releaseType (1=release preferred)
3. Sort by fileDate descending
4. Select first file
```

**Alternative**: Use latestFilesIndexes from mod object

### Pattern: Resolve Dependencies

**Scenario**: Installing JEI with all required dependencies

**Implementation**:
```
1. GET /v1/mods/238222/files/{fileId}
2. Extract dependencies array
3. POST /v1/mods with all dependency modIds
4. For each dependency:
   a. Find compatible file (gameVersion + modLoader)
   b. Extract dependencies recursively
5. Deduplicate, flatten to install list
6. Check for incompatibilities (relationType: 5)
```

### Pattern: Identify Installed Mods

**Scenario**: User has mods folder, identify each mod

**Implementation**:
```
1. For each JAR file:
   a. Compute MurmurHash2 fingerprint
   b. Collect all fingerprints
2. POST /v1/fingerprints/432 with fingerprint array
3. Match exactMatches to original files
4. Handle partialMatches (module-level matching)
5. Report unmatchedFingerprints as unknown
```

### Pattern: Modpack Installation

**Scenario**: Install modpack with 150 mods

**Implementation**:
```
1. Parse modpack manifest (files array)
2. Extract all fileIds
3. POST /v1/mods/files with fileIds (batch)
4. For each file:
   a. Check if already downloaded (cache)
   b. Verify hash if cached
   c. Download if missing/corrupted
5. Resolve transitive dependencies
6. Install in dependency order
```

### Pattern: Update Check

**Scenario**: Check if installed mods have updates

**Implementation**:
```
1. For each installed mod:
   a. GET /v1/mods/{modId}
   b. Check latestFilesIndexes for target version/loader
   c. Compare fileId with installed fileId
   d. If different, update available
2. Batch mod requests with POST /v1/mods
3. Cache mod metadata for 1 hour
4. Display update list to user
```

### Pattern: Fuzzy Modpack Matching

**Scenario**: User has extracted modpack, identify mods

**Implementation**:
```
1. Walk mods/ directory, collect folders
2. For each folder:
   a. Compute fingerprints of all files
   b. Build FolderFingerprint object
3. POST /v1/fingerprints/fuzzy/432
4. Match fuzzyMatches to folders
5. Display identified mods, flag unknowns
```

## API Changelog & Versioning

### Current Version: v1

**Released**: 2022 (Official CurseForge for Studios API)

**Stability**: Stable, production-ready

### Version 2 Endpoints

**Introduced**: Selective v2 endpoints for enhanced responses

**v2 Endpoints**:
- GET /v2/games/{gameId}/versions - Returns structured version objects instead of strings

**Migration**: v1 endpoints remain supported, v2 is optional enhancement

### Breaking Changes

**None Documented**: CurseForge maintains backward compatibility

**Deprecation Policy**: Not publicly documented, monitor developer announcements

### Future Additions

**Not Publicly Roadmapped**: Check CurseForge developer blog and console announcements

**Feature Requests**: Submit via CurseForge Ideas portal

## Support & Resources

### Official Documentation

**API Docs**: https://docs.curseforge.com/rest-api/

**Developer Portal**: https://console.curseforge.com/

**Getting Started**: https://docs.curseforge.com/

### API Key & Support

**API Key Application**: https://console.curseforge.com/ (sign up, apply for key)

**Support Portal**: https://support.curseforge.com/

**Contact**: https://support.curseforge.com/en/support/solutions/articles/9000205544-contact-us

### Terms & Legal

**API Terms**: https://support.curseforge.com/en/support/solutions/articles/9000207405-curse-forge-3rd-party-api-terms-and-conditions

**Platform Terms**: https://legal.overwolf.com/docs/overwolf/platform/platform-terms-of-use/

**Privacy Policy**: https://legal.overwolf.com/docs/overwolf/platform/platform-privacy-policy/

### Status & Monitoring

**Status Page**: https://support.curseforge.com/en/support/solutions/articles/9000205513-status-page

**Outages**: Monitor status page for API availability

### Community Resources

**CurseForge Ideas**: https://curseforge-ideas.overwolf.com/ (feature requests, voting)

**Developer Blog**: https://blog.curseforge.com/

**GitHub Issues**: Community-maintained client libraries often have active issue trackers

### Reference Implementations

**JavaScript/TypeScript**: https://github.com/minimusubi/curseforge-api

**PHP**: https://github.com/aternosorg/php-curseforge-api

**.NET/C#**: https://github.com/CurseForgeCommunity/.NET-APIClient

**Python**: https://pypi.org/project/curseforge/

**Java**: https://github.com/itzg/mc-image-helper (contains CurseForge client)

---

**Document Version**: 1.0.0
**Last Updated**: 2024-12-21
**API Version**: v1 (primary), v2 (selective endpoints)
**Maintainer**: Community-driven reference documentation

**Disclaimer**: This is an unofficial community reference. For authoritative information, consult official CurseForge documentation at https://docs.curseforge.com/. API behavior and limits may change without notice.
