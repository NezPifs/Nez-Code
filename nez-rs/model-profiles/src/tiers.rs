//! Default model selection by available GPU memory.

/// Built-in local model used when nothing else is configured.
pub const DEFAULT_LOCAL_MODEL: &str = "qwen3.5:9b";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardwareTier {
    /// Minimum VRAM in GiB for this tier.
    pub min_vram_gib: u32,
    pub model: &'static str,
    /// One-line explanation shown during setup.
    pub summary: &'static str,
}

/// Tiers from largest to smallest. Q4 weights plus a 16k context must fit in
/// VRAM; spilling to system memory makes agent turns painfully slow.
pub const TIERS: [HardwareTier; 3] = [
    HardwareTier {
        min_vram_gib: 12,
        model: "deepseek-coder-v2:16b",
        summary: "12 GB+ VRAM: dense 16B code model",
    },
    HardwareTier {
        min_vram_gib: 8,
        model: DEFAULT_LOCAL_MODEL,
        summary: "8 GB+ VRAM: Qwen 3.5 9B with native tool calls",
    },
    HardwareTier {
        min_vram_gib: 6,
        model: "qwen2.5-coder:7b-instruct",
        summary: "6 GB VRAM: Qwen 2.5 Coder 7B",
    },
];

/// Recommends a model for the given VRAM in MiB. Unknown VRAM falls back to
/// the default model; anything below 6 GiB gets the smallest tier.
pub fn recommended_model_for_vram(vram_mib: Option<u64>) -> HardwareTier {
    let Some(mib) = vram_mib else {
        return TIERS[1];
    };
    // Drivers report slightly less than the marketed size (an 8 GB card
    // shows ~7.6 GiB), so allow a small margin.
    let gib_with_margin = (mib + 512) / 1024;
    TIERS
        .iter()
        .copied()
        .find(|tier| gib_with_margin >= u64::from(tier.min_vram_gib))
        .unwrap_or(TIERS[2])
}
