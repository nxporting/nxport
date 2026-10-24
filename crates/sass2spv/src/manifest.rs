// Which guest constant buffer, storage buffer or texture handle feeds each binding of a translated stage.

use serde::{Deserialize, Serialize};

use crate::pipeline::ShaderProgram;

fn bool_or_int<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    struct V;
    impl serde::de::Visitor<'_> for V {
        type Value = bool;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a boolean or an integer")
        }
        fn visit_bool<E>(self, v: bool) -> Result<bool, E> {
            Ok(v)
        }
        fn visit_i64<E>(self, v: i64) -> Result<bool, E> {
            Ok(v != 0)
        }
        fn visit_u64<E>(self, v: u64) -> Result<bool, E> {
            Ok(v != 0)
        }
    }
    d.deserialize_any(V)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestCBuffer {
    pub binding: u32,
    pub slot: i32,
    /// Bytes the stage reads at literal offsets; -1 when it indexes the buffer at run time or the extent is unknown.
    #[serde(default = "unknown_extent")]
    pub extent: i32,
}

fn unknown_extent() -> i32 {
    -1
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSBuffer {
    pub binding: u32,
    pub slot: i32,
    pub cb_slot: i32,
    pub cb_offset: i32,
    /// The stage writes it (stores or atomics); manifests without the field count as written.
    #[serde(default = "written")]
    pub write: bool,
}

fn written() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestTextureOrImage {
    pub binding: u32,
    pub cbuf_slot: i32,
    pub handle_index: i32,
    #[serde(rename = "type")]
    pub texture_type: String,
    #[serde(default)]
    pub array_length: i32,
    /// Its first slot in the sampler table (ShaderManifest::sampler_table), or -1.
    #[serde(default = "no_slot")]
    pub sampler_slot: i32,
}

fn no_slot() -> i32 {
    -1
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderManifest {
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default, deserialize_with = "bool_or_int")]
    pub uses_instance_id: bool,
    #[serde(default)]
    pub uses_draw_parameters: bool,
    #[serde(default)]
    pub cbuffers: Vec<ManifestCBuffer>,
    #[serde(default)]
    pub sbuffers: Vec<ManifestSBuffer>,
    #[serde(default)]
    pub textures: Vec<ManifestTextureOrImage>,
    #[serde(default)]
    pub images: Vec<ManifestTextureOrImage>,
    /// Size of the sampler table (properties::SAMPLER_TABLE_BINDING), 0 without one.
    #[serde(default)]
    pub sampler_table: u32,
}

impl ShaderProgram {
    pub fn manifest(&self) -> ShaderManifest {
        let i = &self.info;
        ShaderManifest {
            stage: Some(i.stage.to_string()),
            uses_instance_id: i.uses_instance_id,
            uses_draw_parameters: i.uses_draw_parameters,
            cbuffers: i
                .cbuffers
                .iter()
                .map(|b| ManifestCBuffer {
                    binding: b.binding as u32,
                    slot: b.slot as i32,
                    extent: i.cbuf_extents.iter().find(|e| e.0 == b.binding).map_or(if i.cbuf_extents.is_empty() { -1 } else { 0 }, |e| e.1),
                })
                .collect(),
            sbuffers: i
                .sbuffers
                .iter()
                .map(|b| ManifestSBuffer {
                    binding: b.binding as u32,
                    slot: b.slot as i32,
                    cb_slot: b.sb_cb_slot as i32,
                    cb_offset: b.sb_cb_offset as i32,
                    write: b.write,
                })
                .collect(),
            textures: i
                .textures
                .iter()
                .map(|t| ManifestTextureOrImage {
                    binding: t.binding as u32,
                    cbuf_slot: t.cbuf_slot,
                    handle_index: t.handle_index,
                    texture_type: t.ty.to_string(),
                    array_length: t.array_length,
                    sampler_slot: i
                        .sampler_table
                        .as_ref()
                        .and_then(|(_, b)| b.iter().find(|b| b.0 == t.binding))
                        .map_or(-1, |b| b.1),
                })
                .collect(),
            images: i
                .images
                .iter()
                .map(|t| ManifestTextureOrImage {
                    binding: t.binding as u32,
                    cbuf_slot: t.cbuf_slot,
                    handle_index: t.handle_index,
                    texture_type: t.ty.to_string(),
                    array_length: 0,
                    sampler_slot: -1,
                })
                .collect(),
            sampler_table: i.sampler_table.as_ref().map_or(0, |t| t.0),
        }
    }
}
