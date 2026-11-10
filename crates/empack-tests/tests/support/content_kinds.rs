//! Small, literal archives and independent destinations shared by host and CLI drivers.
use std::io::{Cursor, Write};

pub struct Case {
    pub filename: &'static str,
    pub kind: &'static str,
    pub members: &'static [(&'static str, &'static [u8])],
    pub destination: &'static str,
}

pub const CASES: &[Case] = &[
    Case {
        filename: "mod.jar",
        kind: "mod",
        members: &[("fabric.mod.json", b"{}")],
        destination: "mods/mod.jar",
    },
    Case {
        filename: "resources.zip",
        kind: "resourcepack",
        members: &[("pack.mcmeta", b"{}"), ("assets/example/file", b"resource")],
        destination: "resourcepacks/resources.zip",
    },
    Case {
        filename: "shaders.zip",
        kind: "shader",
        members: &[("shaders/example.glsl", b"shader")],
        destination: "shaderpacks/shaders.zip",
    },
    Case {
        filename: "data.zip",
        kind: "datapack",
        members: &[("pack.mcmeta", b"{}"), ("data/example/file", b"data")],
        destination: "config/paxi/datapacks/data.zip",
    },
];

pub fn archive(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in members {
        archive
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
