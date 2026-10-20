# Running a server distribution

Extract the complete server archive before running its scripts. Keep the scripts
beside `game/`; Minecraft configuration, logs and worlds live inside that directory.
Install the Java version required by the pack. Scripts use `JAVA_HOME/bin/java`
when `JAVA_HOME` is set, otherwise Java on `PATH`.

## Starting the server

| Interface | Windows | macOS and Linux |
| --- | --- | --- |
| Console, without the Minecraft GUI | `start.bat` | `./start.sh` |
| Minecraft GUI | `start_gui.bat` | `./start_gui.sh` |

The default scripts supply `nogui` to Minecraft. The GUI scripts omit that argument;
a GUI still requires a desktop session and support from the selected server runtime.
Both variants pass additional command-line arguments through to the server. For
example, `./start.sh --port 25566` selects another port. These are server arguments,
not JVM options.

On the first run, Minecraft may stop until you review and accept its EULA in
`game/eula.txt`. Empack never accepts the EULA for you.

## Installation and updates

Bundled snapshots already include their pack and runtime files. Their
`install_pack` script confirms that state; use a start script to launch Minecraft.
Reference and empack-managed exports also need `empack` on `PATH`. Both start
variants prepare the exact release before launching and stop if preparation fails.
Subscribed exports check their enrolled update channel before launch.

See [publishing](publishing.md) for publisher enrollment and
[instance commands](usage.md#native-release-example) for manual installation and updates.

## Windows console output

The install and start batch scripts keep their console open when they finish so
errors and EULA messages remain visible. Set `EMPACK_NO_PAUSE=1` for unattended
operation. The scripts preserve the installation or server exit code. If startup
fails, report the preceding console output and the Java version you selected.

## Existing exports and custom scripts

Rebuild a server distribution with v0.7.2-beta or later to receive both start
variants and the default `nogui` argument. Updating the empack executable does not
rewrite an existing ZIP or extracted server.

For snapshot exports, custom scripts in `templates/server/` replace generated
scripts with the same filename. They retain their authored behavior; review them
when changing launch defaults. Subscribed exports require the generated scripts
to preserve preparation and authenticated update checks.
