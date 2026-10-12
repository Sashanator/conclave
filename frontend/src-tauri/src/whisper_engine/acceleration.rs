use crate::audio::{GpuType, PerformanceTier};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperCompiledBackend {
    Metal,
    Cuda,
    Vulkan,
    HipBlas,
    Cpu,
}

impl WhisperCompiledBackend {
    pub fn current() -> Self {
        if cfg!(feature = "cuda") {
            Self::Cuda
        } else if cfg!(feature = "vulkan") {
            Self::Vulkan
        } else if cfg!(feature = "hipblas") {
            Self::HipBlas
        } else if cfg!(target_os = "macos") || cfg!(feature = "metal") {
            Self::Metal
        } else {
            Self::Cpu
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Metal => "Metal",
            Self::Cuda => "Cuda",
            Self::Vulkan => "Vulkan",
            Self::HipBlas => "HipBlas",
            Self::Cpu => "Cpu",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhisperContextAcceleration {
    pub compiled_backend: WhisperCompiledBackend,
    pub runtime_detected_gpu: GpuType,
    pub use_gpu: bool,
    pub flash_attn: bool,
    pub gpu_device: i32,
}

impl WhisperContextAcceleration {
    pub fn status_label(self) -> &'static str {
        match (self.compiled_backend, self.flash_attn) {
            (WhisperCompiledBackend::Metal, true) => "Metal GPU with Flash Attention (Ultra-Fast)",
            (WhisperCompiledBackend::Metal, false) => "Metal GPU acceleration",
            (WhisperCompiledBackend::Cuda, true) => "CUDA GPU with Flash Attention (Ultra-Fast)",
            (WhisperCompiledBackend::Cuda, false) => "CUDA GPU acceleration",
            (WhisperCompiledBackend::Vulkan, _) => "Vulkan GPU acceleration",
            (WhisperCompiledBackend::HipBlas, _) => "HIP BLAS GPU acceleration",
            (WhisperCompiledBackend::Cpu, _) => "CPU processing only",
        }
    }
}

pub fn whisper_context_acceleration_for(
    compiled_backend: WhisperCompiledBackend,
    runtime_detected_gpu: GpuType,
    performance_tier: PerformanceTier,
    cuda_runtime_available: bool,
) -> WhisperContextAcceleration {
    let use_gpu = match compiled_backend {
        WhisperCompiledBackend::Cpu => false,
        WhisperCompiledBackend::Cuda => cuda_runtime_available,
        _ => true,
    };
    let fast_tier = matches!(performance_tier, PerformanceTier::High | PerformanceTier::Ultra);
    let flash_attn = match compiled_backend {
        WhisperCompiledBackend::Metal | WhisperCompiledBackend::Cuda => fast_tier,
        WhisperCompiledBackend::Vulkan | WhisperCompiledBackend::HipBlas | WhisperCompiledBackend::Cpu => false,
    };

    WhisperContextAcceleration {
        compiled_backend,
        runtime_detected_gpu,
        use_gpu,
        flash_attn: use_gpu && flash_attn,
        gpu_device: 0,
    }
}

/// One-line note for the settings UI (spec §4.2).
pub fn backend_label(
    compiled_backend: WhisperCompiledBackend,
    missing_cuda_dlls: &[&str],
    gpu_name: Option<&str>,
) -> String {
    match compiled_backend {
        WhisperCompiledBackend::Cuda if !missing_cuda_dlls.is_empty() => {
            "Whisper running on: CPU — CUDA 12 runtime not found (see CUDA_SETUP.md)".to_string()
        }
        WhisperCompiledBackend::Cuda => match gpu_name {
            Some(name) => format!("Whisper running on: CUDA ({name})"),
            None => "Whisper running on: CUDA".to_string(),
        },
        WhisperCompiledBackend::Cpu => "Whisper running on: CPU".to_string(),
        other => format!("Whisper running on: {}", other.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acceleration_vulkan_backend_ignores_runtime_cuda_flash_attention() {
        let params = whisper_context_acceleration_for(
            WhisperCompiledBackend::Vulkan,
            GpuType::Cuda,
            PerformanceTier::High,
            true,
        );

        assert_eq!(params.compiled_backend, WhisperCompiledBackend::Vulkan);
        assert_eq!(params.runtime_detected_gpu, GpuType::Cuda);
        assert!(params.use_gpu);
        assert!(!params.flash_attn);
    }

    #[test]
    fn acceleration_vulkan_backend_keeps_gpu_without_runtime_gpu_detection() {
        let params = whisper_context_acceleration_for(
            WhisperCompiledBackend::Vulkan,
            GpuType::None,
            PerformanceTier::Low,
            true,
        );

        assert!(params.use_gpu);
        assert!(!params.flash_attn);
    }

    #[test]
    fn acceleration_cuda_backend_enables_flash_attention_for_fast_tiers() {
        let high = whisper_context_acceleration_for(
            WhisperCompiledBackend::Cuda,
            GpuType::Cuda,
            PerformanceTier::High,
            true,
        );
        let ultra = whisper_context_acceleration_for(
            WhisperCompiledBackend::Cuda,
            GpuType::Cuda,
            PerformanceTier::Ultra,
            true,
        );

        assert!(high.use_gpu);
        assert!(high.flash_attn);
        assert!(ultra.use_gpu);
        assert!(ultra.flash_attn);
    }

    #[test]
    fn acceleration_cpu_backend_disables_gpu_and_flash_attention() {
        for runtime_gpu in [GpuType::None, GpuType::Cuda, GpuType::Vulkan] {
            let params = whisper_context_acceleration_for(
                WhisperCompiledBackend::Cpu,
                runtime_gpu,
                PerformanceTier::Ultra,
                true,
            );

            assert!(!params.use_gpu);
            assert!(!params.flash_attn);
        }
    }

    #[test]
    fn acceleration_cuda_backend_without_runtime_falls_back_to_cpu() {
        let params = whisper_context_acceleration_for(
            WhisperCompiledBackend::Cuda,
            GpuType::Cuda,
            PerformanceTier::Ultra,
            false,
        );
        assert!(!params.use_gpu);
        assert!(!params.flash_attn);
    }

    #[test]
    fn label_cuda_with_gpu_name() {
        assert_eq!(
            backend_label(WhisperCompiledBackend::Cuda, &[], Some("NVIDIA GeForce RTX 4070 Ti SUPER")),
            "Whisper running on: CUDA (NVIDIA GeForce RTX 4070 Ti SUPER)"
        );
    }

    #[test]
    fn label_cuda_without_gpu_name() {
        assert_eq!(backend_label(WhisperCompiledBackend::Cuda, &[], None), "Whisper running on: CUDA");
    }

    #[test]
    fn label_cuda_runtime_missing_points_to_setup_guide() {
        assert_eq!(
            backend_label(WhisperCompiledBackend::Cuda, &["cublas64_12.dll"], Some("RTX")),
            "Whisper running on: CPU — CUDA 12 runtime not found (see CUDA_SETUP.md)"
        );
    }

    #[test]
    fn label_cpu_and_other_backends() {
        assert_eq!(backend_label(WhisperCompiledBackend::Cpu, &[], None), "Whisper running on: CPU");
        assert_eq!(backend_label(WhisperCompiledBackend::Vulkan, &[], None), "Whisper running on: Vulkan");
    }
}
