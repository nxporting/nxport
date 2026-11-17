// Building a render pipeline from translated stages.

use super::*;

impl Gfx {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_translated_pipeline(
        &self,
        lib: &ShaderLibs,
        vs: &CachedStage,
        fs: Option<&CachedStage>,
        attrs: &[(u32, MTLVertexFormat, usize, usize, i32)],
        streams_used: &[bool; MAX_STREAMS],
        vertex_streams: &[u64],
        color_pfs: &[MTLPixelFormat],
        depth_pf: MTLPixelFormat,
        depth_gl: bool,
        point_size: f32,
        blends: &[u64; 8],
        color_state: u32,
        channel_mask: u32,
        _blend_color: [f32; 4],
        batched: bool,
        args: bool,
    ) -> Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
        let desc = pipeline_descriptor(
            &self.device,
            lib,
            vs,
            fs,
            attrs,
            streams_used,
            vertex_streams,
            color_pfs,
            depth_pf,
            depth_gl,
            y_negate(),
            point_size,
            blends,
            color_state,
            channel_mask,
            batched,
            args,
        )?;
        super::pipeline_archive::create(&self.device, &desc, &vs.entry, fs.map_or("-", |f| f.entry.as_str()))
    }
}
