//! Central usage accumulation mechanics.

#[derive(Clone, Debug, Default)]
pub(super) struct UsageAccumulator {
    pub(super) input_tokens: Option<u64>,
    pub(super) cached_tokens: Option<u64>,
    pub(super) cache_write_tokens: Option<u64>,
    pub(super) cache_read_tokens: Option<u64>,
    pub(super) cache_creation_tokens: Option<u64>,
    pub(super) cache_write_5m_tokens: Option<u64>,
    pub(super) cache_write_1h_tokens: Option<u64>,
    pub(super) output_tokens: Option<u64>,
    pub(super) reasoning_tokens: Option<u64>,
    pub(super) total_tokens: Option<u64>,
}

pub(super) fn merge_option(target: &mut Option<u64>, value: Option<u64>) {
    if value.is_some() {
        *target = value;
    }
}
