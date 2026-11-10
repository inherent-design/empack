# Java selection

Java requirements belong to the selected runtime, not the author's installed JVM. Empack keeps the historical game minimum, its finite admitted majors, the highest admitted recommendation, the author's preference, and the actual executable separate. A policy ceiling is a support boundary; it is not proof that every newer Java fails.

## Authoring and resolution

Author schema 6 expresses a preference explicitly:

```yaml
runtime:
  minecraft: "1.21.1"
  loader:
    kind: neoforge
    pin: "21.1.200"
  java:
    preferred: 21
```

The preference selects a major within the admitted set. It does not narrow the whole pack to that major: a 1.17.1 profile admitting 16 and 17 can prefer 17 while accepting 16. Known loader, content, or author constraints can narrow a profile; they cannot erase a hard minimum. Unknown compatibility requires an explicit preference and remains unqualified. A different installed major is not accepted automatically for that unknown profile.

Initialization and import display the recommendation and admitted set after runtime selection. Interactive creation offers the recommended major first and allows another admitted major. Noninteractive creation uses the recommendation unless `--java` supplies an override. An unknown exact runtime needs an explicit choice. Missing local Java does not block authoring that does not execute it.

Runtime changes preserve the existing preference unless the author selects a replacement. An incompatible retained preference is an error with guidance to review `migrate --java`; upgrading the game does not silently change author intent. Schema 5's scalar `runtime.java` meant a minimum, so it is rejected rather than reinterpreted. Lock schema 3 records the resolved policy identity and profile; an incompatible policy requires review and synchronization.

## Finite policy

`empack-core::java` owns the pure selection and constraint rules. Its checked-in exact-version profiles come from Prism's `compatibleJavaMajors`, with hashes checked against the pinned metadata index. Normal authoring and launch perform no policy download.

| Release era                      | Historical game minimum | Initial admitted majors | Recommended |
| -------------------------------- | ----------------------: | ----------------------- | ----------: |
| Supported releases through 1.5.2 |                       5 | 8                       |           8 |
| 1.6.1 to 1.11.2                  |                       6 | 8                       |           8 |
| 1.12 to 1.16.5                   |                       8 | 8                       |           8 |
| 1.17 to 1.17.1                   |                      16 | 16, 17                  |          17 |
| 1.18 to 1.20.4                   |                      17 | 17                      |          17 |
| 1.20.5 to 1.21.11                |                      21 | 21                      |          21 |
| Known 26.1 to 26.3 releases      |                      25 | 25                      |          25 |

The table summarizes eras, not a prefix matcher. The checked-in profile list owns actual coverage. Selected snapshots and special releases have exact entries; an unlisted release, prerelease, snapshot, experiment, or custom identifier is unknown. No SemVer or string-order assumption extends support to a future identifier. Historical Java 5/6 requirements do not add JVM execution support below empack's Java 8 floor.

The [Minecraft Wiki](https://minecraft.wiki/w/Tutorial:Update_Java) describes historical minimums. Mojang documents [Java 16](https://feedback.minecraft.net/hc/en-us/articles/360060771772-Minecraft-Java-Edition-Snapshot-21w19a), [Java 17](https://feedback.minecraft.net/hc/en-us/articles/4415128577293-Minecraft-Java-Edition-1-18), [Java 21](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-5), and [Java 25](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-1) transitions. Initial conservative support comes from [Prism metadata revision b8e2a3e](https://github.com/PrismLauncher/meta-launcher/tree/b8e2a3e1ba241414f8b861f13c9a6fd6bc5d922d/net.minecraft). These inputs do not certify a complete modpack, JVM vendor, update level, architecture, native library, or launch flag combination. Development-JDK recommendations alone do not establish a loader's runtime maximum.

## Execution and distribution

Release schema 2 carries the preferred major, required minimum, finite supported majors or explicit unknown state, and policy identity. A recipient validates actual Java against those requirements. Verified server metadata can raise the minimum and narrow admitted majors, but cannot silently replace an incompatible author preference.

Generated server scripts use `JAVA_HOME/bin/java` when configured, otherwise `java` on PATH. Author-side Forge-family installers use the host execution configuration; their Java process is separate from recipient selection. Empack does not install or remove JDKs, change machine registration, or infer recipient requirements from that host executable. Prism remains responsible for selecting Java and obtaining client runtime components. Generated standalone bundled scripts check the resolved admitted majors before invoking the game, without an empack dependency. Custom standalone entry points remain author-owned and are not certified by that check.

Process retirement, terminal restoration, persisted ownership cleanup, and the final command result retain their existing contracts. A Java refusal must not start the game or suppress recovery diagnostics. An admitted major is not a gameplay certificate.

## Maintenance

Inspect exact upstream metadata and loader/runtime evidence before changing coverage. `python3 scripts/update-java-policy.py --check` checks the retained profiles against their pinned index and file hashes; it is an opt-in contributor network command. `--revision COMMIT` selects a new immutable metadata snapshot. Review the resulting profile changes, historical classifications, affected consumers, and regression cases before updating the policy identity. Do not generate policy by observing which JVM happens to be installed.

Pure tests cover exact boundaries, unknown versions, lower-major overrides, exclusions, and constraint intersections. Host and executable tests must separately cover prompts, document preservation, release round trips, actual JVM probes, and final diagnostics. A fixture or help/EULA boundary does not establish real launcher or interactive Minecraft acceptance.
