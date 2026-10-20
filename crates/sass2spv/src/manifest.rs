use serde::{Deserialize, Serialize};

fn deserialize_bool_or_int<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct BoolOrIntVisitor;
    impl<'de> serde::de::Visitor<'de> for BoolOrIntVisitor {
        type Value = bool;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a boolean or an integer")
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
    deserializer.deserialize_any(BoolOrIntVisitor)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestCBuffer {
    pub binding: u32,
    pub slot: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSBuffer {
    pub binding: u32,
    pub slot: i32,
    pub cb_slot: i32,
    pub cb_offset: i32,
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
}

/// The binding manifest emitted alongside the compiled SPIR-V (by the C# decompiler or the Rust port).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderManifest {
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default, deserialize_with = "deserialize_bool_or_int")]
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
}

impl From<ShaderProgramInfo> for ShaderManifest {
    fn from(info: ShaderProgramInfo) -> Self {
        Self {
            stage: Some(info.stage),
            uses_instance_id: info.uses_instance_id,
            uses_draw_parameters: info.uses_draw_parameters,
            cbuffers: info
                .cbuffers
                .into_iter()
                .map(|c| ManifestCBuffer {
                    binding: c.binding,
                    slot: c.slot,
                })
                .collect(),
            sbuffers: info
                .sbuffers
                .into_iter()
                .map(|s| ManifestSBuffer {
                    binding: s.binding,
                    slot: s.slot,
                    cb_slot: s.cb_slot,
                    cb_offset: s.cb_offset,
                })
                .collect(),
            textures: info
                .textures
                .into_iter()
                .map(|t| ManifestTextureOrImage {
                    binding: t.binding,
                    cbuf_slot: t.cbuf_slot,
                    handle_index: t.handle_index,
                    texture_type: t.texture_type,
                    array_length: t.array_length,
                })
                .collect(),
            images: info
                .images
                .into_iter()
                .map(|i| ManifestTextureOrImage {
                    binding: i.binding,
                    cbuf_slot: i.cbuf_slot,
                    handle_index: i.handle_index,
                    texture_type: i.image_type,
                    array_length: 0,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CBufferInfo {
    pub binding: u32,
    pub slot: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SBufferInfo {
    pub binding: u32,
    pub slot: i32,
    pub cb_slot: i32,
    pub cb_offset: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureInfo {
    pub binding: u32,
    pub cbuf_slot: i32,
    pub handle_index: i32,
    #[serde(rename = "type")]
    pub texture_type: String,
    pub array_length: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    pub binding: u32,
    pub cbuf_slot: i32,
    pub handle_index: i32,
    #[serde(rename = "type")]
    pub image_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderProgramInfo {
    pub stage: String,
    pub uses_instance_id: bool,
    pub uses_draw_parameters: bool,
    pub code_end: usize,
    pub attribute_usage: crate::decoders::decoder::AttributeUsage,
    pub cbuffers: Vec<CBufferInfo>,
    pub sbuffers: Vec<SBufferInfo>,
    pub textures: Vec<TextureInfo>,
    pub images: Vec<ImageInfo>,
}

impl ShaderProgramInfo {
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn extract_from_program(
        program: &crate::decoders::decoder::DecodedProgram,
        accessor: &mut crate::gpu_accessor::MemoryGpuAccessor,
        stage_name: &str,
    ) -> Self {
        use crate::decoders::inst_decoders::*;
        use crate::decoders::inst_name::InstName;
        use rustc_hash::FxHashSet;

        let mut used_cbuffers: FxHashSet<i32> = FxHashSet::default();
        let mut used_textures: FxHashSet<i32> = FxHashSet::default();

        for block in &program.blocks {
            for op in &block.op_codes {
                match op.name {
                    InstName::Ldc => {
                        let ldc = InstLdc::new(op.raw_op_code);
                        used_cbuffers.insert(ldc.cbuf_slot());
                    }
                    InstName::Tex => {
                        let tex = InstTex::new(op.raw_op_code);
                        used_textures.insert(tex.tid_b());
                    }
                    InstName::Texs => {
                        let texs = InstTexs::new(op.raw_op_code);
                        used_textures.insert(texs.tid_b());
                    }
                    InstName::Tld4 => {
                        let tld4 = InstTld4::new(op.raw_op_code);
                        used_textures.insert(tld4.tid_b());
                    }
                    InstName::Tld4s => {
                        let tld4s = InstTld4s::new(op.raw_op_code);
                        used_textures.insert(tld4s.tid_b());
                    }
                    _ => {}
                }
            }
        }

        let mut cbuffers = Vec::new();
        let mut sorted_cbufs: Vec<i32> = used_cbuffers.into_iter().collect();
        sorted_cbufs.sort_unstable();

        for slot in sorted_cbufs {
            let pair = accessor.create_constant_buffer_binding(slot);
            cbuffers.push(CBufferInfo {
                binding: pair.binding,
                slot,
            });
        }

        let mut textures = Vec::new();
        let mut sorted_textures: Vec<i32> = used_textures.into_iter().collect();
        sorted_textures.sort_unstable();

        for handle in sorted_textures {
            let pair = accessor.create_texture_binding(1);
            textures.push(TextureInfo {
                binding: pair.binding,
                cbuf_slot: 0,
                handle_index: handle,
                texture_type: "Texture2D".to_string(),
                array_length: 0,
            });
        }

        Self {
            stage: stage_name.to_string(),
            uses_instance_id: true,
            uses_draw_parameters: false,
            code_end: 0x30 + program.max_end_address as usize,
            attribute_usage: program.attribute_usage,
            cbuffers,
            sbuffers: vec![],
            textures,
            images: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_manifest_deserialization() {
        let json = r#"{"stage":"Vertex","usesInstanceId":true,"usesDrawParameters":false,"codeEnd":2960,"cbuffers":[{"binding":0,"slot":-1},{"binding":1,"slot":1}],"sbuffers":[{"binding":2,"slot":-1,"cbSlot":-1,"cbOffset":32}],"textures":[{"binding":3,"cbufSlot":-1,"handleIndex":-1,"type":"Texture2D","arrayLength":0}],"images":[{"binding":5,"cbufSlot":0,"handleIndex":6,"type":"Image2D"}]}"#;
        let manifest: ShaderManifest = serde_json::from_str(json).expect("deserialize C# manifest");
        assert_eq!(manifest.stage.as_deref(), Some("Vertex"));
        assert!(manifest.uses_instance_id);
        assert!(!manifest.uses_draw_parameters);
        assert_eq!(manifest.cbuffers.len(), 2);
        assert_eq!(manifest.cbuffers[0].slot, -1);
        assert_eq!(manifest.sbuffers.len(), 1);
        assert_eq!(manifest.sbuffers[0].slot, -1);
        assert_eq!(manifest.textures.len(), 1);
        assert_eq!(manifest.textures[0].handle_index, -1);
        assert_eq!(manifest.images.len(), 1);
    }
}

