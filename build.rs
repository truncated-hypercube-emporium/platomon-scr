//! With the `gpu` feature: compile the Space heater compute shader from WGSL
//! to SPIR-V 1.3 (Vulkan 1.1), validated here so a shader error is a build
//! error, not a run-time one.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "gpu")]
    gpu::compile();
}

#[cfg(feature = "gpu")]
mod gpu {
    use std::path::PathBuf;

    const SHADER: &str = "src/gpu/frame.wgsl";

    pub fn compile() {
        println!("cargo:rerun-if-changed={SHADER}");
        let src = std::fs::read_to_string(SHADER).expect("read the shader");
        let module = match naga::front::wgsl::parse_str(&src) {
            Ok(m) => m,
            Err(e) => panic!("{SHADER}: {}", e.emit_to_string(&src)),
        };
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{SHADER}: {}", e.emit_to_string(&src)));
        let options = naga::back::spv::Options {
            // SPIR-V 1.3 = Vulkan 1.1: what a Radeon HD 7850 (GCN 1.0) runs
            lang_version: (1, 3),
            flags: naga::back::spv::WriterFlags::empty(),
            ..Default::default()
        };
        let pipeline = naga::back::spv::PipelineOptions { shader_stage: naga::ShaderStage::Compute, entry_point: "main".into() };
        let words = naga::back::spv::write_vec(&module, &info, &options, Some(&pipeline)).expect("write SPIR-V");
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("frame.spv");
        std::fs::write(out, bytes).expect("write frame.spv");
    }
}
